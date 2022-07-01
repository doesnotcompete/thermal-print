# `thermal-print`
![Crates.io](https://img.shields.io/crates/v/thermal-print?style=flat-square)
![Crates.io](https://img.shields.io/crates/l/thermal-print?style=flat-square)

## Summary
`thermal-print` provides a serial interface driver for the ESC/POS implementation of the CSN-A2 thermal printer sold by [Adafruit](https://www.adafruit.com/product/597) and others. The crate should be supported on all platforms targeted by `embedded-hal` which possess a dynamic allocator, and it is `#![no_std]`-compatible.

## Functionality
`thermal-print` still lacks some minor functionality, but already supports

 - [x] text formatting (such as justification, selecting a print mode, and choosing fonts),
 - [x] printing barcodes,
 - [x] bitmap printing (via the `tinybmp` crate).

## Usage
**Minimum Supported Rust Version:** 1.56.0

Just depend on the crate in your Cargo manifest:
```
[dependencies]
thermal-print = "0.1"
```

Now you can bring the crate into scope:
```
use thermal_print::*
```

Bitmap printing depends on the [TinyBMP](https://crates.io/crates/tinybmp) crate. See the example on printing bitmaps below.

## Examples
### Setup, Formatting, and Printing Text
This example initializes the printer, configures font `B`, sets the justification to center and prints `Hello, world!` onto the paper.

```
  // Configure the serial interface for your platform
  let config = serial::config::Config::default()
      .baudrate(Hertz(19_200));
  let mut serial: serial::Serial<serial::UART1, _, _> = serial::Serial::new(
      peripherals.uart1,
      serial::Pins {
          tx: peripherals.pins.gpio21,
          rx: peripherals.pins.gpio19,
          cts: None,
          rts: None,
      },
      config
    ).expect("Error while configuring UART!");

  // Construct a new `Printer` with the serial interface and a `delay` implementation for 
  // blocking while the printer prints
  let mut printer = Printer::new(serial, delay::FreeRtos);
  printer.init();
  
  printer.set_print_mode(
    PrintModeBuilder::default()
      .font(Font::FontB)
      .build()
      .unwrap()
    );
  printer.set_justification(Justification::Center);

  writeln!(printer, "Hello, world!");
```

### Printing Bitmaps
This example prints a bitmap embedded via the `include_bytes!` macro. It assumes you have added `tinybmp` as a dependency to your manifest, and that a suitable bitmap file `./resources/ferris.bmp` is present in your project. See the documentation on further information on bitmap printing.

```
use tinybmp::RawBmp;

printer.print_bitmap(
  RawBmp::from_slice(
    include_bytes!("../resources/ferris.bmp")
  ).unwrap(),
  RasterBitImageMode::Normal
);
```

## Feature Flags
 - `std`: This enables linking against the Rust standard library. It is _disabled_ by default.
