//! Raspberry Pi 4 demo.
//! This example makes use the `std` feature
//! and `anyhow` dependency to make error handling more ergonomic.
//!
//! # Connections
//!
//! - 3V3    = VCC
//! - GND    = GND
//! - GPIO9  = MISO
//! - GPIO10 = MOSI
//! - GPIO11 = SCLK (SCK)
//! - GPIO22 = NSS  (SDA)

use linux_embedded_hal as hal;

use std::fs::File;
use std::io::Write;

use anyhow::Result;
use embedded_hal::blocking::delay::DelayMs;
use embedded_hal::blocking::spi::{Transfer as SpiTransfer, Write as SpiWrite};
use embedded_hal::digital::v2::OutputPin;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::sysfs_gpio::Direction;
use hal::{Delay, Pin, Spidev};
use mfrc522::{Mfrc522, Initialized, WithNssDelay};

// NOTE this requires tweaking permissions and configuring LED0
//
// ```
// $ echo gpio | sudo tee /sys/class/leds/led0/trigger
// $ sudo chown root:gpio /sys/class/leds/led0/brightness
// $ sudo chmod 770 /sys/class/leds/led0/brightness
// ```
//
// Alternatively you can omit the LED and comment out the contents of the `on` and `off` methods
pub struct Led;

impl Led {
    fn on(&mut self) {
        File::create("/sys/class/leds/led0/brightness")
            .unwrap()
            .write_all(b"1\n")
            .unwrap();
    }

    fn off(&mut self) {
        File::create("/sys/class/leds/led0/brightness")
            .unwrap()
            .write_all(b"0\n")
            .unwrap();
    }
}

fn main() -> Result<()> {
    let mut led = Led;
    let mut delay = Delay;

    let mut spi = Spidev::open("/dev/spidev0.0").unwrap();
    let options = SpidevOptions::new()
        .max_speed_hz(1_000_000)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options).unwrap();

    // software-controlled chip select pin
    let pin = Pin::new(22);
    pin.export().unwrap();
    while !pin.is_exported() {}
    delay.delay_ms(1u32); // delay sometimes necessary because `is_exported()` returns to early?
    pin.set_direction(Direction::Out).unwrap();
    pin.set_value(1).unwrap();

    // The `with_nss` method provides a GPIO pin to the driver for software controlled chip select.
    let mut mfrc522 = Mfrc522::new(spi).with_nss(pin).init()?;

    let vers = mfrc522.version()?;

    println!("VERSION: 0x{:x}", vers);

    assert!(vers == 0x91 || vers == 0x92);

    loop {
        const CARD_UID: [u8; 4] = [34, 246, 178, 171];
        const TAG_UID: [u8; 4] = [128, 170, 179, 76];

        if let Ok(atqa) = mfrc522.reqa() {
            if let Ok(uid) = mfrc522.select(&atqa) {
                println!("UID: {:?}", uid.as_bytes());

                if uid.as_bytes() == &CARD_UID {
                    led.off();
                    println!("CARD");
                } else if uid.as_bytes() == &TAG_UID {
                    led.on();
                    println!("TAG");
                }

                handle_authenticate(&mut mfrc522, &uid, |m| {
                    let data = m.mf_read(1, true)?;
                    println!("read {:?}", data);
                    Ok(())
                })
                .ok();
            }
        }

        delay.delay_ms(1000u32);
    }
}

fn handle_authenticate<E, SPI, NSS, D, F>(
    mfrc522: &mut Mfrc522<SPI, NSS, D, Initialized>,
    uid: &mfrc522::Uid,
    action: F,
) -> Result<()>
where
    SPI: SpiTransfer<u8, Error = E> + SpiWrite<u8, Error = E>,
    Mfrc522<SPI, NSS, D, Initialized>: WithNssDelay,
    F: FnOnce(&mut Mfrc522<SPI, NSS, D, Initialized>) -> Result<()>,
    E: std::fmt::Debug + std::marker::Sync + std::marker::Send + 'static,
{
    // Use *default* key, this should work on new/empty cards
    let key = [0xFF; 6];
    if mfrc522.mf_authenticate(uid, 1, &key).is_ok() {
        action(mfrc522)?;
    } else {
        println!("Could not authenticate");
    }

    mfrc522.hlta()?;
    mfrc522.stop_crypto1()?;
    Ok(())
}
