#![no_std]
#![no_main]

use bme280::i2c::AsyncBME280;
use defmt::*;
use embassy_executor::{Spawner, task};
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::i2c::I2c;
use embassy_stm32::mode::Async;
use embassy_stm32::rcc::{
    Hsi48Config, LsConfig, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllRDiv, PllSource,
};
use embassy_stm32::time::{Hertz, khz};
use embassy_stm32::usart::{self};
use embassy_stm32::{Config, bind_interrupts, i2c, peripherals, spi};
use embassy_time::{Delay, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::LoRa;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::mod_params::{Bandwidth, CodingRate, SpreadingFactor};
use lora_phy::sx126x::{self, Sx126x, Sx1262, TcxoCtrlVoltage};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    USART2 => usart::InterruptHandler<peripherals::USART2>;
});

const LORA_FREQUENCY_IN_HZ: u32 = 918_000_000;

#[unsafe(no_mangle)]
unsafe extern "Rust" fn _embassy_trace_task_exec_begin(executor: u32, task: u32) {
    trace!("_embassy_trace_task_exec_begin: ({},{})", executor, task);
}

#[unsafe(no_mangle)]
unsafe extern "Rust" fn _embassy_trace_task_exec_end(executor: u32, task: u32) {
    trace!("_embassy_trace_task_exec_end: ({},{})", executor, task);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn _embassy_trace_executor_idle(executor: u32) {
    trace!("_embassy_trace_executor_idle: ({})", executor);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn _embassy_trace_task_ready_begin(executor: u32, task: u32) {
    trace!("_embassy_trace_task_ready_begin: ({},{})", executor, task);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn _embassy_trace_task_new(executor: u32, task: u32) {
    trace!("_embassy_trace_task_new: ({},{})", executor, task);
}

#[task]
async fn get_atmo_data(mut bme280: AsyncBME280<I2c<'static, Async>>) -> ! {
    let mut delay = embassy_time::Delay;
    bme280.init(&mut delay).await.unwrap();

    let mut last_data = Instant::now();

    loop {
        // measure temperature, pressure, and humidity
        let measurements = bme280.measure(&mut delay).await.unwrap();
        let previous_time = last_data;
        last_data = Instant::now();

        let diff = last_data - previous_time;
        let frequency = 1 * 1_000_000 / diff.as_micros();
        let data_rate = frequency * (2 * (8 * 4)); // Sending 2 32bit floats every x microseconds
        info!(
            "Diff: {}, Frequency: {}, Datarate: {}bps",
            diff.as_micros(),
            frequency,
            data_rate
        );

        info!(
            "Temperature = {} deg C, Pressure = {} pascals",
            measurements.temperature, measurements.pressure
        );
    }
}

async fn spew_data(
    spi: ExclusiveDevice<spi::Spi<'static, Async>, Output<'static>, Delay>,
    iv: GenericSx126xInterfaceVariant<Output<'static>, ExtiInput<'static>>,
) {
    info!("Beginning Spew");
    let config = sx126x::Config {
        chip: Sx1262,
        rx_boost: true,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
        use_dcdc: true,
    };
    let mut lora = LoRa::new(Sx126x::new(spi, iv, config), false, Delay)
        .await
        .unwrap();

    info!("Init OK");

    let mdltn_params = {
        match lora.create_modulation_params(
            SpreadingFactor::_10,
            Bandwidth::_250KHz,
            CodingRate::_4_8,
            LORA_FREQUENCY_IN_HZ,
        ) {
            Ok(mp) => mp,
            Err(err) => {
                info!("Radio error = {:?}", err);
                return;
            }
        }
    };

    let mut tx_pkt_params = {
        match lora.create_tx_packet_params(4, false, true, false, &mdltn_params) {
            Ok(pp) => pp,
            Err(err) => {
                info!("Radio error = {:?}", err);
                return;
            }
        }
    };
    let mut last_data = Instant::now();
    let mut iterator = 0u64;
    let mut buffer: [u8; 3 + 8] = [0; 11];

    loop {
        let header = [0x01u8, 0x02u8, 0x00u8];

        for (i, byte) in header.iter().enumerate() {
            buffer[i] = *byte;
        }
        for (i, byte) in iterator.to_be_bytes().iter().enumerate() {
            buffer[i + 3] = *byte;
        }
        iterator += 1;

        match lora
            .prepare_for_tx(&mdltn_params, &mut tx_pkt_params, 22, &buffer)
            .await
        {
            Ok(()) => {}
            Err(err) => {
                info!("Radio error = {:?}", err);
                return;
            }
        };

        match lora.tx().await {
            Ok(()) => {}
            Err(err) => {
                info!("Radio error = {:?}", err);
                return;
            }
        };

        info!("Transmit Success");
        let previous_time = last_data;
        last_data = Instant::now();

        let diff = last_data - previous_time;
        let frequency = 1 * 1_000_000 / diff.as_micros();
        let data_rate = frequency * (8 * 11); // Sending 11 bytes floats every x microseconds
        info!(
            "Diff: {}, Frequency: {}, Datarate: {}bps",
            diff.as_micros(),
            frequency,
            data_rate
        );
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut cfg = Config::default();

    cfg.rcc.ls = LsConfig::default_lse();

    cfg.rcc.hsi48 = Some(Hsi48Config::default());
    cfg.rcc.hsi = true;

    cfg.rcc.pll = Some(Pll {
        source: PllSource::HSI,
        prediv: PllPreDiv::DIV1,
        mul: PllMul::MUL10,
        divp: Some(PllPDiv::DIV7),
        divq: Some(PllQDiv::DIV2),
        divr: Some(PllRDiv::DIV2),
    });

    let p = embassy_stm32::init(cfg);
    info!("Hello World!");

    let mut led = Output::new(p.PB13, Level::High, Speed::Low);

    let i2c = I2c::new(
        p.I2C1,
        p.PB6,
        p.PB7,
        Irqs,
        p.DMA2_CH7,
        p.DMA2_CH6,
        Hertz(100_000),
        Default::default(),
    );
    let bme280 = bme280::i2c::AsyncBME280::new(i2c, 0x76);

    // let mut config = Config::default();
    // config.baudrate = 38400;
    // let gps = Uart::new(
    //     p.USART2,
    //     p.PA3,
    //     p.PA2,
    //     Irqs,
    //     p.DMA1_CH7,
    //     p.DMA1_CH6,
    //     config,
    // )
    // .unwrap();
    // let mut driver = ublox_core::new_serial_driver(gps);
    // driver.setup(&mut Delay).unwrap();

    let nss = Output::new(p.PB2, Level::High, Speed::Low);
    let reset = Output::new(p.PC4, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PC5, p.EXTI5, Pull::Down);
    let busy = ExtiInput::new(p.PA4, p.EXTI4, Pull::Up);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).expect("Should be able to create SPI");
    let iv = GenericSx126xInterfaceVariant::new(reset, irq, busy, None, None).unwrap();

    // spawner.spawn(get_atmo_data(bme280)).unwrap();
    // spawner.spawn(spew_data(spi, iv)).unwrap();
    Timer::after_secs(2).await;

    spew_data(spi, iv).await;

    loop {
        // let rc = driver.handle_one_message();
        // if let Ok(msg_count) = rc {
        //     if msg_count > 0 {
        //         if let Some(nav_pvt) = driver.take_last_nav_pvt() {
        //             info!(
        //                 ">>> nav_pvt {} lat, lon: {}, {} \r\n",
        //                 nav_pvt.itow, nav_pvt.lat, nav_pvt.lon,
        //             );
        //         }
        //         if let Some(nav_dop) = driver.take_last_nav_dop() {
        //             info!(">>> nav_dop {} \r\n", nav_dop.itow);
        //         }
        //         if let Some(mon_hw) = driver.take_last_mon_hw() {
        //             info!(">>> mon_hw jam: {} \r\n", mon_hw.jam_ind);
        //         }
        //     } else {
        //         info!("No Message");
        //     }
        // } else {
        //     println!(">>> {} \r\n", defmt::Debug2Format(&rc));
        // }
        led.set_high();
        Timer::after_millis(300).await;
        led.set_low();
        Timer::after_millis(300).await;
    }
}
