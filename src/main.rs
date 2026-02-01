use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sys::{
    esp_deep_sleep_enable_gpio_wakeup, esp_deep_sleep_start, esp_now_init,
    esp_now_register_recv_cb, esp_now_recv_info_t, esp_sleep_get_wakeup_cause,
    esp_sleep_wakeup_cause_t_ESP_SLEEP_WAKEUP_GPIO, gpio_int_type_t_GPIO_INTR_HIGH_LEVEL,
    gpio_wakeup_enable, ESP_OK,
};
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use log::info;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

// --- Topics ---
const TOPIC_ID_KETTLE_THERMO: i32 = 1;
const TOPIC_ID_SINK_THERMO: i32 = 2;
#[allow(dead_code)]
const TOPIC_ID_KETTLE_SOUND: i32 = 3;

// --- Pin Definitions ---
// GPIO 34-39 are input only on ESP32
// Using gpio32 for THERMO_1_LED (Kettle Thermostat)
// Using gpio12 for THERMO_2_LED (Sink Thermostat)
// Using gpio13 for BUZZER
const WAKEUP_GPIO: i32 = 4;

// --- Data structure matching the sender ---
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct HubData {
    topic_id: i32,
    measurement: i32,
}

// FreeRTOS Implementation
// Static variables for received data (used in callback)
static RECEIVED_TOPIC: AtomicI32 = AtomicI32::new(-1);
static RECEIVED_MEASUREMENT: AtomicI32 = AtomicI32::new(0);
static DATA_READY: AtomicBool = AtomicBool::new(false);

// RTC slow memory to persist across deep sleep
// Note: In esp-idf-svc, we use a static with #[link_section] for RTC memory
#[link_section = ".rtc.data"]
static mut IS_AWAKE: bool = false;

/// ESP-NOW receive callback
unsafe extern "C" fn on_receive(
    _info: *const esp_now_recv_info_t,
    data: *const u8,
    len: core::ffi::c_int,
) {
    if len as usize == std::mem::size_of::<HubData>() {
        let hub_data: HubData = std::ptr::read_unaligned(data as *const HubData);
        info!(
            "Received - Topic ID: {} | Measurement: {}",
            hub_data.topic_id, hub_data.measurement
        );

        RECEIVED_TOPIC.store(hub_data.topic_id, Ordering::SeqCst);
        RECEIVED_MEASUREMENT.store(hub_data.measurement, Ordering::SeqCst);
        DATA_READY.store(true, Ordering::SeqCst);
    }
}

fn go_to_sleep() {
    info!("Going to sleep now");
    unsafe {
        IS_AWAKE = false;
        esp_deep_sleep_start();
    }
}

fn sound_buzzer(buzzer: &mut PinDriver<'_, impl esp_idf_svc::hal::gpio::OutputPin, esp_idf_svc::hal::gpio::Output>) {
    buzzer.set_high().ok();
    FreeRtos::delay_ms(500);
    buzzer.set_low().ok();
}

fn flash_led(led: &mut PinDriver<'_, impl esp_idf_svc::hal::gpio::OutputPin, esp_idf_svc::hal::gpio::Output>) {
    led.set_high().ok();
    FreeRtos::delay_ms(1000);
    led.set_low().ok();
}

fn main() {
    // Link ESP-IDF patches
    esp_idf_svc::sys::link_patches();

    // Initialize logger
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("ESP-NOW Receiver Starting...");

    // Get peripherals
    let peripherals = Peripherals::take().unwrap();

    // Check wakeup cause
    let wakeup_reason = unsafe { esp_sleep_get_wakeup_cause() };

    unsafe {
        if wakeup_reason == esp_sleep_wakeup_cause_t_ESP_SLEEP_WAKEUP_GPIO {
            if IS_AWAKE {
                info!("Sensor touched: Going to sleep");
                go_to_sleep();
            } else {
                info!("Sensor touched: Waking up");
                IS_AWAKE = true;
            }
        } else {
            info!("Normal Boot");
            IS_AWAKE = false;
        }
    }

    // Configure GPIO outputs
    let mut thermo_1_led = PinDriver::output(peripherals.pins.gpio32).unwrap();
    let mut thermo_2_led = PinDriver::output(peripherals.pins.gpio12).unwrap();
    let mut buzzer = PinDriver::output(peripherals.pins.gpio13).unwrap();

    // Ensure all outputs start LOW
    thermo_1_led.set_low().ok();
    thermo_2_led.set_low().ok();
    buzzer.set_low().ok();

    // Configure wakeup GPIO
    let _wakeup_pin = PinDriver::input(peripherals.pins.gpio4).unwrap();

    // Initialize WiFi in STA mode (required for ESP-NOW)
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap(),
        sys_loop,
    )
    .unwrap();

    // Set WiFi to STA mode without connecting
    wifi.set_configuration(&esp_idf_svc::wifi::Configuration::Client(
        esp_idf_svc::wifi::ClientConfiguration::default(),
    ))
    .unwrap();
    wifi.start().unwrap();

    info!("WiFi started in STA mode");

    // Enable GPIO wakeup
    unsafe {
        gpio_wakeup_enable(
            WAKEUP_GPIO as u32,
            gpio_int_type_t_GPIO_INTR_HIGH_LEVEL,
        );
        esp_deep_sleep_enable_gpio_wakeup(
            1u64 << WAKEUP_GPIO,
            esp_idf_svc::sys::gpio_deepsleep_wakeup_level_t_ESP_GPIO_WAKEUP_GPIO_HIGH,
        );
    }

    // Initialize ESP-NOW
    unsafe {
        if esp_now_init() != ESP_OK as i32 {
            info!("ESP-NOW Init Failed");
            return;
        }
        info!("ESP-NOW Initialized");

        if IS_AWAKE {
            esp_now_register_recv_cb(Some(on_receive));
            info!("ESP-NOW receive callback registered");
        }
    }

    // Main loop
    loop {
        if DATA_READY.load(Ordering::SeqCst) {
            let topic_id = RECEIVED_TOPIC.load(Ordering::SeqCst);
            let measurement = RECEIVED_MEASUREMENT.load(Ordering::SeqCst);

            info!("Processing - Topic ID: {} | Measurement: {}", topic_id, measurement);

            match topic_id {
                TOPIC_ID_KETTLE_THERMO => {
                    if measurement > 50 {
                        sound_buzzer(&mut buzzer);
                        flash_led(&mut thermo_1_led);
                    } else {
                        thermo_1_led.set_low().ok();
                        buzzer.set_low().ok();
                    }
                }
                TOPIC_ID_SINK_THERMO => {
                    if measurement < 32 {
                        sound_buzzer(&mut buzzer);
                        flash_led(&mut thermo_2_led);
                    } else {
                        thermo_2_led.set_low().ok();
                        buzzer.set_low().ok();
                    }
                }
                _ => {
                    thermo_1_led.set_low().ok();
                    thermo_2_led.set_low().ok();
                    buzzer.set_low().ok();
                }
            }

            DATA_READY.store(false, Ordering::SeqCst);
        }

        FreeRtos::delay_ms(10);
    }
}
