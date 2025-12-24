#![deny(unsafe_code)]
#![no_main]
#![no_std]


use cortex_m::singleton;
use embedded_hal::digital::{
    OutputPin,
    StatefulOutputPin,
};
use ghostfat;
use heapless::Vec;
use panic_halt as _;
use rtic_monotonics::systick::prelude::*;
use rtic_sync::{channel::*, make_channel};
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
use usb_device::{class_prelude::*, prelude::*};
// use usbd_mass_storage::USB_CLASS_MSC;
use usbd_scsi::Scsi;
use usbd_serial::{SerialPort, USB_CLASS_CDC};
use usbd_storage::{
    // subclass::{
    //     scsi::{Scsi, ScsiCommand},
    //     Command,
    // },
    // transport::{
    //     bbb::{BulkOnly, BulkOnlyError},
    //     TransportError,
    // },
    CLASS_MASS_STORAGE,
};
use ws2812_pio::Ws2812;

use web_serial_blinky::adc_rand_seed::adc_seed;
use web_serial_blinky::garland::{
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
    type Files = [ghostfat::File<'static, 512>; 2];
    type Storage = Scsi<'static, hal::usb::UsbBus, ghostfat::GhostFat<'static>>;


    #[shared]
    struct Shared {
        serial: Serial,
    }

    #[local]
    struct Local {
        adc: hal::Adc,
        led: Led,
        rgb_led: RgbLed,
        usb_dev: UsbDev,
        storage: Storage,
        action_sender: Sender<'static, LedAction, 1>,
    }

    #[init]
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
        let usb_bus: &'static mut _ = singleton!(: UsbBus = usb_bus).unwrap();

        // Set up the USB Communications Class Device driver
        let serial = SerialPort::new(usb_bus);

        // Virtual files in GhostFAT
        let readme = b"Nothing to see here!\nMind your own business!";
        // let file: ghostfat::File<> = ghostfat::File::new("README.txt", data).unwrap();
        let control = include_bytes!("../web_interface/control.htm");
        // let data = include_bytes!("../web_interface/test_include.txt");
        let readme: ghostfat::File<> = ghostfat::File::new("readme.md ", readme).unwrap();
        let control: ghostfat::File<> = ghostfat::File::new("control.htm", control).unwrap();
        let files: &'static mut _ = singleton!(: Files = [control, readme]).unwrap();

        let mut config: ghostfat::Config<> = ghostfat::Config::default();
        config.volume_label = "Blinky";
        let ghost_fat = ghostfat::GhostFat::new(
            files,
            config,
        );

        let storage = Scsi::new(
            usb_bus, 
            64,
            ghost_fat,
            "yuri",
            "Web blinky",
            "",
        );

        let usb_dev = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0x0011, 0x7788))
            .strings(&[StringDescriptors::default()
                .manufacturer("yuri")
                .product("Web blinky")])
            .unwrap()
            .device_class(0x00)
            .build();


        let (action_sender, action_receiver) = make_channel!(LedAction, 1);
        let (status_sender, status_receiver) = make_channel!(LedStatus, 1);

        led::spawn(action_receiver, status_sender).ok();
        rgb_led::spawn().ok();
        status::spawn(status_receiver).ok();

        (
            Shared {
                serial,
            },
            Local {
                adc,
                led,
                rgb_led,
                usb_dev,
                storage,
                action_sender,
            },
        )
    }


    #[task(binds = USBCTRL_IRQ, local = [usb_dev, storage, action_sender],
        shared = [serial], priority = 1)]
    fn usb(cx: usb::Context) {

        let usb::LocalResources
            {usb_dev, storage, action_sender, ..} = cx.local;

        let usb::SharedResources
            {mut serial, ..} = cx.shared;

        let ON_MES = b"LED_ON";
        let OFF_MES = b"LED_OFF";
        let TOGGLE_MES = b"LED_TOGGLE";

        serial.lock(|serial| {
            if usb_dev.poll(&mut [serial, storage]) {
                let mut buf = [0u8; 64];
                match serial.read(&mut buf) {
                    Err(_e) => {}
                    Ok(0) => {}
                    Ok(count) => {

                        if count >= ON_MES.len() && &buf[..ON_MES.len()] == ON_MES {
                            action_sender.try_send(LedAction::ON).ok();
                        } else if count >= OFF_MES.len() && &buf[..OFF_MES.len()] == OFF_MES {
                            action_sender.try_send(LedAction::OFF).ok();
                        } else if count >= TOGGLE_MES.len() && &buf[..TOGGLE_MES.len()] == TOGGLE_MES {
                            action_sender.try_send(LedAction::TOGGLE).ok();
                        }
                    }
                }
            }
        })
    }

    // Control on-board LED
    #[task(local = [led], priority = 1)]
    async fn led(cx: led::Context,
        mut action_receiver: Receiver<'static, LedAction, 1>,
        mut status_sender: Sender<'static, LedStatus, 1>,
    ) {

        let led::LocalResources
            {led, ..} = cx.local;

        let mut status = LedStatus::OFF;

        let mut set_led = |st: &LedStatus|{
            match st {
                LedStatus::OFF => {
                    led.set_low().ok();
                }
                LedStatus::ON => {
                    led.set_high().ok();
                }
            }
        };

        loop {

            let action = action_receiver.recv().await.unwrap();
            match action {
                LedAction::OFF => {
                    status = LedStatus::OFF;
                    set_led(&status);
                }
                LedAction::ON => {
                    status = LedStatus::ON;
                    set_led(&status);
                }
                LedAction::TOGGLE => {
                    status = if status == LedStatus::ON {LedStatus::OFF}
                        else {LedStatus::ON};
                    set_led(&status);
                }
            }
            status_sender.send(status).await.ok();
        }
    }

    // Send LED status responce
    #[task(shared = [serial], priority = 1)]
    async fn status(cx: status::Context,
        mut status_receiver: Receiver<'static, LedStatus, 1>,
    ) {

        let status::SharedResources
            {mut serial, ..} = cx.shared;

        loop {
            let status = status_receiver.recv().await.unwrap();

            serial.lock(|serial| {
                match status {
                    LedStatus::ON => {
                        serial.write(b"LED_IS_ON\n").ok();
                    }
                    LedStatus::OFF => {
                        serial.write(b"LED_IS_OFF\n").ok();
                    }
                }
            })
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

enum LedAction {
    ON,
    OFF,
    TOGGLE,
}

#[derive(PartialEq, Copy, Clone)]
enum LedStatus {
    ON,
    OFF,
}
