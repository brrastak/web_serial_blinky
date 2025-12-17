#![deny(unsafe_code)]
#![no_main]
#![no_std]


use core::mem::MaybeUninit;
use embedded_hal::digital::{
    OutputPin,
    StatefulOutputPin,
};
use heapless::Vec;
use panic_halt as _;
use rtic_monotonics::systick::prelude::*;
use tinyrand::{StdRand, RandRange, Seeded};
use smart_leds::{SmartLedsWrite, RGB8};
use vcc_gnd_yd_rp2040 as rp;
use rp::{
    Pins,
    XOSC_CRYSTAL_FREQ
};
use rp::hal as hal;
use hal::{
        clocks::{init_clocks_and_plls, Clock},
        pio::PIOExt,
        Sio,
        timer::Timer,
        watchdog::Watchdog,
    };
use usbd_serial::{SerialPort, USB_CLASS_CDC};
use usb_device::{class_prelude::*, prelude::*};
use ws2812_pio::Ws2812;

use webusb_blinky::adc_rand_seed::adc_seed;
use webusb_blinky::garland::{
    AMPLITUDE,
    no_pastel,
    triangle_wave,
};


systick_monotonic!(Mono, 1000);


#[rtic::app(device = hal::pac, peripherals = true, dispatchers = [I2C0_IRQ])]
mod app {

    use super::*;


    type RgbLed = Ws2812<hal::pac::PIO0, hal::pio::SM0, hal::timer::CountDown, hal::gpio::Pin<hal::gpio::bank0::Gpio23, hal::gpio::FunctionPio0, hal::gpio::PullDown>>;
    type Led = hal::gpio::Pin<hal::gpio::bank0::Gpio25, hal::gpio::FunctionSioOutput, hal::gpio::PullDown,>;
    type UsbBus = UsbBusAllocator<hal::usb::UsbBus>;
    type UsbDev = UsbDevice<'static, hal::usb::UsbBus>;
    type Serial = SerialPort<'static, hal::usb::UsbBus>;


    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        adc: hal::Adc,
        led: Led,
        rgb_led: RgbLed,
        usb_dev: UsbDev,
        serial: Serial,
    }

    #[init (local = [
        usb_bus: MaybeUninit<UsbBus> = MaybeUninit::uninit(),
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {

        let mut resets = cx.device.RESETS;
        let mut watchdog = Watchdog::new(cx.device.WATCHDOG);
        let clocks = init_clocks_and_plls(
            XOSC_CRYSTAL_FREQ,
            cx.device.XOSC,
            cx.device.CLOCKS,
            cx.device.PLL_SYS,
            cx.device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        let sio = Sio::new(cx.device.SIO);
        let pins = Pins::new(
            cx.device.IO_BANK0,
            cx.device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );

        Mono::start(cx.core.SYST, clocks.system_clock.freq().to_Hz());

        let mut led = pins.led.into_push_pull_output();
        led.set_low().unwrap();

        let delay = Timer::new(cx.device.TIMER, &mut resets, &clocks);

        // Configure the addressable LED
        let (mut pio, sm0, _, _, _) = cx.device.PIO0.split(&mut resets);
        let rgb_led = Ws2812::new(
            pins.neopixel.into_function(),
            &mut pio,
            sm0,
            clocks.peripheral_clock.freq(),
            delay.count_down(),
        );

        // ADC to get random seed
        let adc = hal::Adc::new(cx.device.ADC, &mut resets);


        // Set up the USB driver
        let usb_bus = UsbBusAllocator::new(hal::usb::UsbBus::new(
            cx.device.USBCTRL_REGS,
            cx.device.USBCTRL_DPRAM,
            clocks.usb_clock,
            true,
            &mut resets,
        ));
        let usb_bus: &'static mut _ = cx.local.usb_bus.write(usb_bus);

        // Set up the USB Communications Class Device driver
        let serial = SerialPort::new(usb_bus);

        // Create a USB device with a fake VID and PID
        let usb_dev = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0x16c0, 0x27dd))
            .strings(&[StringDescriptors::default()
                .manufacturer("Fake company")
                .product("Serial port")
                .serial_number("TEST")])
            .unwrap()
            .device_class(USB_CLASS_CDC)
            .build();

        
        heartbeat::spawn().ok();
        rgb_led::spawn().ok();

        (
            Shared {},
            Local {
                adc,
                led,
                rgb_led,
                usb_dev,
                serial,
            },
        )
    }


    #[task(binds = USBCTRL_IRQ, local = [usb_dev, serial], priority = 1)]
    fn usb(cx: usb::Context) {

        let usb::LocalResources
            {usb_dev, serial, ..} = cx.local;

        if usb_dev.poll(&mut [serial]) {
            let mut buf = [0u8; 64];
            match serial.read(&mut buf) {
                Err(_e) => {
                    // Do nothing
                }
                Ok(0) => {
                    // Do nothing
                }
                Ok(count) => {
                    // Convert to upper case
                    buf.iter_mut().take(count).for_each(|b| {
                        b.make_ascii_uppercase();
                    });
                    // Send back to the host
                    let mut wr_ptr = &buf[..count];
                    while !wr_ptr.is_empty() {
                        match serial.write(wr_ptr) {
                            Ok(len) => wr_ptr = &wr_ptr[len..],
                            // On error, just drop unwritten data.
                            // One possible error is Err(WouldBlock), meaning the USB
                            // write buffer is full.
                            Err(_) => break,
                        };
                    }
                }
            }
        }
    }

    // Blink on-board LED
    #[task(local = [led], priority = 1)]
    async fn heartbeat(cx: heartbeat::Context) {

        let heartbeat::LocalResources
            {led, ..} = cx.local;

        loop {
            led.toggle().ok();
            Mono::delay(1000.millis()).await;
        }
    }

    // Generate set of colors for RGB LED using random color
    #[task(local = [adc, rgb_led], priority = 1)]
    async fn rgb_led(cx: rgb_led::Context) {

        let rgb_led::LocalResources
            {adc, rgb_led, ..} = cx.local;

        let mut temp_sensor = adc.take_temp_sensor().unwrap();
        let mut adc = adc.build_fifo().set_channel(&mut temp_sensor).start();
        let mut adc_seed_values: Vec<u16, 64> = Vec::from_slice(&[0u16; 64]).unwrap();
        for value in adc_seed_values.iter_mut() {
            *value = adc.read_single();
        }

        let seed = adc_seed(adc_seed_values);
        let mut rand = StdRand::seed(seed as u64);

        loop {
            
            let color = RGB8 {
                r: rand.next_range(0..AMPLITUDE) as u8,
                g: rand.next_range(0..AMPLITUDE) as u8,
                b: rand.next_range(0..AMPLITUDE) as u8,
            };
            let color = no_pastel(color);
            let pattern = triangle_wave(color);

            for color in pattern {

                rgb_led.write([color]).ok();
                Mono::delay(50.millis()).await;
            }
        }
    }


    #[idle]
    fn idle(_: idle::Context) -> ! {

        loop {
            continue;
        }
    }
}
