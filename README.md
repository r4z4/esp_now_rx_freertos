 I see you're currently using esp-idf-svc which uses FreeRTOS under the hood. To avoid FreeRTOS entirely, we need to
  switch to the bare-metal approach using esp-hal and esp-wifi crates.

Key mappings from Arduino to Rust:
┌─────────────────────────────────────┬───────────────────────────────────────────────────┐
│ Arduino │ Rust │
├─────────────────────────────────────┼───────────────────────────────────────────────────┤
│ HubData struct │ #[repr(C, packed)] struct HubData │
├─────────────────────────────────────┼───────────────────────────────────────────────────┤
│ RTC_DATA_ATTR bool isAwake │ #[link_section = ".rtc.data"] static mut IS_AWAKE │
├─────────────────────────────────────┼───────────────────────────────────────────────────┤
│ esp_now_register_recv_cb(onReceive) │ esp_now_register_recv_cb(Some(on_receive)) │
├─────────────────────────────────────┼───────────────────────────────────────────────────┤
│ digitalWrite(pin, HIGH/LOW) │ pin.set_high()/set_low() │
├─────────────────────────────────────┼───────────────────────────────────────────────────┤
│ delay(ms) │ FreeRtos::delay_ms(ms) │
├─────────────────────────────────────┼───────────────────────────────────────────────────┤
│ WiFi.mode(WIFI_STA) │ BlockingWifi with ClientConfiguration │
└─────────────────────────────────────┴───────────────────────────────────────────────────┘
Key differences in the Rust implementation:

1. Callback handling: The ESP-NOW callback uses atomic variables (AtomicI32, AtomicBool) to safely pass data from the
   interrupt context to the main loop, rather than directly calling GPIO functions in the callback.
2. Type safety: GPIO pins are type-checked at compile time through PinDriver.
3. Memory safety: RTC memory is placed using #[link_section = ".rtc.data"] instead of RTC_DATA_ATTR.
4. Struct alignment: The HubData struct uses #[repr(C, packed)] to match the C struct layout from your sender.

To build and flash:
cargo build --release
espflash flash target/xtensa-esp32-espidf/release/esp_now_receiver

Make sure your sender's HubData struct uses the same field order (topicId then measurement), both as int (4 bytes
each).
