#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{Config, spi};
use embassy_time::Delay;
use embedded_hal_async::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Config::default());

    let nss = Output::new(p.PB2, Level::High, Speed::Low);
    let mut busy = ExtiInput::new(p.PA4, p.EXTI4, Pull::Down);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
    );
    let mut spi = ExclusiveDevice::new(spi, nss, Delay).expect("Should be able to create SPI");

    loop {
        info!("Waiting for low");
        spi.write(&[0xC0u8, 0x0u8]).await.unwrap();
        busy.wait_for_low().await;
        info!("Got low");
    }
}
