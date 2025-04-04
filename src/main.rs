#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::interrupt::Priority;
use embassy_stm32::rcc::{Pll, Sysclk};
use embassy_stm32::time::mhz;
use embassy_stm32::{Config, spi};
use embassy_time::{Delay, Timer};
use embedded_hal_async::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = Config::default();

    config.enable_debug_during_sleep = true;
    config.bdma_interrupt_priority = Priority::P2;

    config.rcc.hsi = true;
    config.rcc.pll = Some(Pll {
        source: embassy_stm32::rcc::PllSource::HSI,
        prediv: embassy_stm32::rcc::PllPreDiv::DIV1,
        mul: embassy_stm32::rcc::PllMul::MUL10,
        divp: None,
        divq: Some(embassy_stm32::rcc::PllQDiv::DIV2),
        divr: Some(embassy_stm32::rcc::PllRDiv::DIV4)
    });
    config.rcc.sys = Sysclk::PLL1_R;

    let p = embassy_stm32::init(config);

    let nss = Output::new(p.PB2, Level::High, Speed::VeryHigh);
    let mut busy = ExtiInput::new(p.PC4, p.EXTI4, Pull::Up);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = mhz(10);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
    );
    let mut spi = ExclusiveDevice::new(spi, nss, Delay).expect("Should be able to create SPI");

    loop {
        info!("Writing Buf");
        spi.write(&[0xC0u8, 0x0u8]).await.expect("SPI Write should suceed");
        info!("Wrote buf");
        busy.wait_for_low().await;
        Timer::after_millis(10).await
    }
}
