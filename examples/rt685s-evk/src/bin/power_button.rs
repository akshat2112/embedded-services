#![no_std]
#![no_main]

extern crate embedded_services_examples;

use defmt::info;
use embassy_executor::Spawner;
use embassy_imxrt::gpio::{self, Input, Inverter, Pull};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::once_lock::OnceLock;
use embassy_sync::signal::Signal;
use embassy_time::Duration;
use embedded_services::transport::{self, Endpoint, Internal};
use power_button_service::button::{Button, ButtonConfig};
use power_button_service::button_interpreter::{check_button_press, Message};
use power_button_service::debounce::{ActiveState, Debouncer};
use {defmt_rtt as _, panic_probe as _};

mod sender {
    use super::*;

    pub struct Sender {
        pub tp: transport::EndpointLink,
        sn: Signal<ThreadModeRawMutex, Message>,
    }

    impl Sender {
        pub fn new() -> Self {
            Self {
                tp: transport::EndpointLink::uninit(Endpoint::Internal(Internal::Power)),
                sn: Signal::new(),
            }
        }

        pub async fn send(&self, message: Message) {
            self.tp
                .send(Endpoint::Internal(Internal::Power), &message)
                .await
                .unwrap();
        }
    }

    impl<'a> transport::MessageDelegate for Sender {
        fn process(&self, message: &transport::Message) {
            if let Some(sig) = message.data.get::<Message>() {
                self.sn.signal(*sig);
            }
        }
    }
}

mod receiver {
    use super::*;

    pub struct Receiver {
        pub tp: transport::EndpointLink,
        pub sn: Signal<ThreadModeRawMutex, Message>,
    }

    impl Receiver {
        pub fn new() -> Self {
            Self {
                tp: transport::EndpointLink::uninit(Endpoint::Internal(Internal::Power)),
                sn: Signal::new(),
            }
        }
    }

    impl transport::MessageDelegate for Receiver {
        fn process(&self, message: &transport::Message) {
            if let Some(sig) = message.data.get::<Message>() {
                self.sn.signal(*sig);
            }
        }
    }
}

#[embassy_executor::task(pool_size = 4)]
async fn button_task(gpio: Input<'static>, config: ButtonConfig) {
    static SENDER: OnceLock<sender::Sender> = OnceLock::new();
    let sender = SENDER.get_or_init(|| sender::Sender::new());
    let mut button = Button::new(gpio, config);

    loop {
        match check_button_press(&mut button).await {
            Some(Message::ShortPress) => {
                info!("Short press");
                sender.send(Message::ShortPress).await;
            }
            Some(Message::LongPress) => {
                info!("Long press");
                sender.send(Message::LongPress).await;
            }
            Some(Message::PressAndHold) => {
                info!("Press and hold");
                sender.send(Message::PressAndHold).await;
            }
            None => {}
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    embedded_services::init().await;

    let p = embassy_imxrt::init(Default::default());

    unsafe { gpio::init() };

    // Create a power button instance
    let button_a = Input::new(p.PIO1_1, Pull::Up, Inverter::Disabled);
    // Create a debouncer instance
    let debouncer = Debouncer::new(3, Duration::from_millis(10), ActiveState::ActiveLow);
    // Create a custom button configuration instance
    let config_a = ButtonConfig::new(debouncer, Duration::from_millis(1000), Duration::from_millis(2000));

    // Create a second button instance
    let button_b = Input::new(p.PIO0_10, Pull::Up, Inverter::Disabled);
    // Create a default button configuration instance
    let config_b = ButtonConfig::default();

    // Spawn the button tasks
    spawner.must_spawn(button_task(button_a, config_a));
    spawner.must_spawn(button_task(button_b, config_b));

    static RECEIVER: OnceLock<receiver::Receiver> = OnceLock::new();
    let receiver = RECEIVER.get_or_init(receiver::Receiver::new);

    transport::register_endpoint(receiver, &receiver.tp).await.unwrap();

    // Create an LED instance
    let mut led_r = gpio::Output::new(
        p.PIO0_31,
        gpio::Level::Low,
        gpio::DriveMode::PushPull,
        gpio::DriveStrength::Normal,
        gpio::SlewRate::Standard,
    );

    // Create an LED instance
    let mut led_g = gpio::Output::new(
        p.PIO0_14,
        gpio::Level::Low,
        gpio::DriveMode::PushPull,
        gpio::DriveStrength::Normal,
        gpio::SlewRate::Standard,
    );

    // Create an LED instance
    let mut led_b = gpio::Output::new(
        p.PIO0_26,
        gpio::Level::Low,
        gpio::DriveMode::PushPull,
        gpio::DriveStrength::Normal,
        gpio::SlewRate::Standard,
    );

    loop {
        let msg = receiver.sn.wait().await;

        // Toggle the LEDs based on the button press duration
        match msg {
            Message::ShortPress => {
                led_g.toggle();
            }
            Message::LongPress => {
                led_b.toggle();
            }
            Message::PressAndHold => {
                led_r.toggle();
            }
        }
    }
}
