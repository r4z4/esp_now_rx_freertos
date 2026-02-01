Here using esp-idf-svc which uses FreeRTOS under the hood. To avoid FreeRTOS entirely, we need to switch to the bare-metal approach using esp-hal and esp-wifi crates.

To build and flash:
cargo build --release
espflash flash target/xtensa-esp32-espidf/release/esp_now_receiver

Make sure sender's HubData struct uses the same field order (topicId then measurement), both as int (4 bytes
each).
