/*  This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <http://www.gnu.org/licenses/>.
*/

//! Support for the CSN-A2 thermal printer via `embedded_hal`.
//!
//! # Usage
//! Create a new [`Printer`] on a serial port on your platform and write text via the implemented `core::fmt::Write` trait. You can use the `write!` and `writeln!` macros to accomplish this.
//!
//! See the [`Printer`] struct documentation for advanced capabilities, such as printing barcodes and
//! bitmaps.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "alloc"))]
extern crate alloc;

use alloc::format;
use bitvec::prelude::*;
use core::fmt::{Arguments, Error, Write};
use core::iter::zip;

use derive_builder::Builder;
use embedded_hal::{blocking::delay, serial};
use num_enum::IntoPrimitive;
use tinybmp::RawBmp;

const ESC: u8 = 0x1B; // Escape
const HT: u8 = 0x09; // Horizontal tab
const MARK: u8 = 0x21; // !
const AT: u8 = 0x40; // @
const GS: u8 = 0x1D;

const INIT_SEQUENCE: [u8; 2] = [ESC, AT];
const TAB_STOP_SEQUENCE: [u8; 2] = [ESC, b'D'];
const MODE_SEQUENCE: [u8; 2] = [ESC, MARK];
/// Modes: Inverse, Upside-Down, Underline
const MODE_ORDER: [[u8; 2]; 3] = [[GS, 0x42], [ESC, 0x7B], [ESC, 0x45]];

const TAB_WIDTH: u8 = 4;
const PIXEL_COLOR_CUTOFF: u32 = 0x0000FFFF;
const BAUDRATE: u64 = 19_200;
/// Maximum number of horizontal dots the printer can handle
const DOT_WIDTH: u32 = 384;
/// Time estimate for the printer to process one byte of data
const BYTE_TIME_MICROS: u64 = ((11 * 1000000) + (BAUDRATE / 2)) / BAUDRATE;

#[derive(Clone, Copy)]
pub enum Font {
    FontA,
    FontB,
}

impl Default for Font {
    fn default() -> Self {
        Self::FontA
    }
}

pub enum Justification {
    Left,
    Center,
    Right,
}

impl Default for Justification {
    fn default() -> Self {
        Self::Left
    }
}

pub enum Underline {
    None,
    Normal,
    Double,
}

impl Default for Underline {
    fn default() -> Self {
        Self::None
    }
}

/// Defines the raster bit-image mode.
///
/// | Mode           | Vertical Density  | Horizontal Density    |
/// |----------------|-------------------|-----------------------|
/// | `Normal`       | 203.2dpi          | 203.2dpi              |
/// | `DoubleWidth`  | 203.2dpi          | 101.6dpi              |
/// | `DoubleHeight` | 101.6dpi          | 203.2dpi              |
/// | `Quadruple`    | 101.6dpi          | 101.6dpi              |
///
#[derive(IntoPrimitive)]
#[repr(u8)]
pub enum RasterBitImageMode {
    Normal = 0,
    DoubleWidth,
    DoubleHeight,
    Quadruple,
}

impl Default for RasterBitImageMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(IntoPrimitive)]
#[repr(u8)]
pub enum CharacterSet {
    USA = 0,
    France,
    Germany,
    UK,
    DenmarkI,
    Sweden,
    Italy,
    SpainI,
    Japan,
    Norway,
    DenmarkII,
    SpainII,
    LatinAmerica,
    Korea,
    SloveniaCroatia,
    China,
}

impl Default for CharacterSet {
    fn default() -> Self {
        Self::USA
    }
}

#[derive(IntoPrimitive)]
#[repr(u8)]
pub enum CodeTable {
    CP437 = 0,
    Katakana = 1,
    CP850 = 2,
    CP860 = 3,
    CP863 = 4,
    CP865 = 5,
    WCP1251 = 6,
    CP866 = 7,
    MIK = 8,
    CP755 = 9,
    Iran = 10,
    CP862 = 15,
    WCP1252 = 16,
    WCP1253 = 17,
    CP852 = 18,
    CP858 = 19,
    IranII = 20,
    Latvian = 21,
    CP864 = 22,
    Iso8859_1 = 23,
    CP737 = 24,
    WCP1257 = 25,
    Thai = 26,
    CP720 = 27,
    CP855 = 28,
    CP857 = 29,
    WCP1250 = 30,
    CP775 = 31,
    WCP1254 = 32,
    WCP1255 = 33,
    WCP1256 = 34,
    WCP1258 = 35,
    Iso8859_2 = 36,
    Iso8859_3 = 37,
    Iso8859_4 = 38,
    Iso8859_5 = 39,
    Iso8859_6 = 40,
    Iso8859_7 = 41,
    Iso8859_8 = 42,
    Iso8859_9 = 43,
    Iso8859_15 = 44,
    Thai2 = 45,
    CP856 = 46,
    CP874 = 47,
}

impl Default for CodeTable {
    fn default() -> Self {
        Self::CP437
    }
}

/// Defines the barcode system to be used. Some systems are considered binary-level, and some are
/// multi-level systems, which is important for setting the barcode width. See [`BarcodeWidth`] for
/// more information.
#[derive(IntoPrimitive)]
#[repr(u8)]
pub enum BarCodeSystem {
    UpcA = 65,
    UpcE = 66,
    Ean13 = 67,
    Ean8 = 68,
    Code39 = 69,
    Itf = 70,
    Codabar = 71,
    Code93 = 72,
    Code128 = 73,
}

impl Default for BarCodeSystem {
    fn default() -> Self {
        Self::UpcA
    }
}

/// These are special characters described in the printer documentation.
///
/// TODO: Find out what exactly this is useful for.
#[derive(IntoPrimitive)]
#[repr(u8)]
pub enum BarCodeSpecialCharacter {
    Shift = 0x53,
    CodeA = 0x41,
    CodeB = 0x42,
    CodeC = 0x43,
    Fnc1 = 0x31,
    Fnc2 = 0x32,
    Fnc3 = 0x33,
    Fnc4 = 0x34,
    /// "{"
    CurlyOpen = 0x7B,
}

/// See the table below for barcode width options depending on the barcode type.
/// The default is `Width3`.
///
/// | Width     | Module Width (mm) for multi-level barcode | Thin Element (mm) for binary-level | Thick Element (mm) for binary-level |
/// | `Width2`  | 0.250                                     | 0.250                              | 0.625                               |
/// | `Width3`  | 0.375                                     | 0.375                              | 1.000                               |
/// | `Width4`  | 0.560                                     | 0.560                              | 1.250                               |
/// | `Width5`  | 0.625                                     | 0.625                              | 1.625                               |
/// | `Width6`  | 0.750                                     | 0.750                              | 2.000                               |
///
/// `UpcA`, `UpcE`, `Ean8`, `Ean13` `Code93` and `Code128` are considered multi-level barcodes.
/// `Code39`, `Itf` and `Codabar` are binary-level codes.
#[derive(IntoPrimitive)]
#[repr(u8)]
pub enum BarcodeWidth {
    Width2 = 2,
    Width3 = 3,
    Width4 = 4,
    Width5 = 5,
    Width6 = 6,
}

#[derive(Default, Builder, Clone, Copy)]
#[builder(default, setter(into), no_std)]
pub struct PrintMode {
    font: Font,
    inverse: bool,
    upside_down: bool,
    emph: bool,
    double_height: bool,
    double_width: bool,
    delete_line: bool,
}

/// Defines the printer's heat settings.
#[derive(Builder, Clone, Copy)]
#[builder(default, setter(into), no_std)]
pub struct PrintSettings {
    /// The maximum number of print head elements that will fire simultaneously. More dots require
    /// a higher peak current, but speed up the printing process. Unit: 8 dots.
    /// Default: 11 (96 dots)
    dots: u8,
    /// The duration that the heat elements are fired. A longer heating time results in a darker
    /// print, but a slower printing speed. Unit: 10 microseconds. Default: 1.2
    /// milliseconds.
    time: u8,
    /// The recovery time between firing of the heating dots. A longer recovery interval results in
    /// clearer prints, but a slower printing speed and possibly static friction between the paper
    /// and the print roll. Unit: 10 microseconds. Default: 200
    /// microseconds.
    interval: u8,
}

impl Default for PrintSettings {
    fn default() -> Self {
        PrintSettings {
            dots: 11,
            time: 120,
            interval: 20,
        }
    }
}

impl Into<u8> for PrintMode {
    fn into(self) -> u8 {
        let mut mode = 0;

        match self.font {
            Font::FontA => mode &= 1 << 0,
            Font::FontB => mode |= 1 << 0,
        }

        // Somehow this seems to be broken (some bit-flags are ignored); use the custom (mode-specific) commands defined in the datasheet instead
        if self.inverse {
            mode |= 1 << 1;
        }

        if self.upside_down {
            mode |= 1 << 2;
        }

        if self.emph {
            mode |= 1 << 3;
        }

        if self.double_height {
            mode |= 1 << 4;
        }

        if self.double_width {
            mode |= 1 << 5;
        }

        if self.delete_line {
            mode |= 1 << 6;
        }
        mode
    }
}

impl Into<[u8; 3]> for PrintSettings {
    fn into(self) -> [u8; 3] {
        [self.dots, self.time, self.interval]
    }
}

/// A representation of the thermal printer. Implements the `core::fmt::Write` trait for printing
/// normal text.
pub struct Printer<Port: serial::Write<u8>, Delay: delay::DelayUs<u32>> {
    pub serial: Port,
    pub delay: Delay,
    prev_byte: char,
    max_column: u8,
    char_height: u8,
    char_width: u8,
    line_spacing: u8,
    barcode_height: u8,
    dot_print_time: u32,
    dot_feed_time: u32,
    current_column: u8,
    print_mode: u8,
}

impl<Port: serial::Write<u8>, Delay: delay::DelayUs<u32>> Printer<Port, Delay> {
    /// Create a new `Printer` with default settings.
    pub fn new(serial: Port, delay: Delay) -> Printer<Port, Delay> {
        Printer {
            serial,
            delay,
            prev_byte: '\n',
            max_column: 32,
            char_height: 24,
            char_width: 12,
            line_spacing: 6,
            barcode_height: 162,
            dot_print_time: 0,
            dot_feed_time: 0,
            current_column: 0,
            print_mode: 0,
        }
    }

    /// Lower-level function to directly write an array of bytes to the output sink. Wraps around
    /// [`write_byte`].
    ///
    /// Functions producing physical output on the printer should use [`write`] instead.
    fn write_bytes(&mut self, bytes: &[u8]) {
        for b in bytes.iter() {
            self.write_byte(*b).unwrap();
        }
    }

    /// Write a single byte to the underlying serial output.
    ///
    /// Functions producing physical output on the printer should use [`write_one`] instead.
    fn write_byte(&mut self, byte: u8) -> Result<(), ()> {
        let result = self.serial.write(byte);
        self.sleep(BYTE_TIME_MICROS);
        match result {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    /// Writes multiple bytes to the printer. Wraps around [`write_one`].
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes.iter() {
            self.write_one(*b).unwrap();
        }
    }

    /// Write a single byte to the printer, keeping track of the physical position of the print
    /// head and blocking accordingly. Control commands should be issued via [`write_byte`]
    /// instead.
    fn write_one(&mut self, byte: u8) -> Result<(), ()> {
        let result = self.serial.write(byte);

        // To keep up with the physical hardware, we try to estimate the time it takes for the
        // printer to output what we're sending it
        let mut wait_duration: u64 = BYTE_TIME_MICROS;

        // Check if we're encountering a line break
        if byte == b'\n' || self.current_column == self.max_column {
            let char_height: u64 = self.char_height.into();
            let line_spacing: u64 = self.line_spacing.into();
            let dot_feed_time: u64 = self.dot_feed_time.into();
            let dot_print_time: u64 = self.dot_print_time.into();
            if self.prev_byte == '\n' {
                // Just a feed line
                wait_duration += (char_height + line_spacing) * dot_feed_time;
            } else {
                // We still have characters to print
                wait_duration += (char_height * dot_print_time) + (line_spacing * dot_feed_time);
            }
            self.current_column = 0;
            self.prev_byte = '\n';
        } else {
            // Check if this is a tab
            if byte == HT {
                let next_column: u8 = (self.current_column / TAB_WIDTH) + 1;
                self.current_column += next_column;
            }
            self.current_column += 1;
            self.prev_byte = byte as char;
        }
        self.sleep(wait_duration);
        match result {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    /// Halt the program for the specified number of microseconds. We don't want to overrun the
    /// printer's buffer, so this function is used to wait for the print head to physically produce
    /// the desired output.
    fn sleep(&mut self, duration: u64) {
        self.delay.delay_us(duration as u32);
    }

    /// Configure the tab stop widths on the printer.
    fn update_tabs(&mut self) {
        self.write_bytes(&TAB_STOP_SEQUENCE);
        for i in 1..(self.max_column / TAB_WIDTH) {
            self.write_byte(i * TAB_WIDTH).unwrap();
        }
        self.write_byte(0x00).unwrap();
    }

    /// Send the initialization sequence and configure tab stops.
    pub fn reset(&mut self) {
        // Init
        self.write_bytes(&INIT_SEQUENCE);

        // Configure tab stops
        self.update_tabs();
        self.set_print_settings(PrintSettings::default());
        self.set_character_set(CharacterSet::default());
        self.set_code_table(CodeTable::default());
        self.set_barcode_height(self.barcode_height);
    }

    /// Wake the device from sleep. Also block for 75ms, as according to the datasheet the
    /// printer needs at least 50ms in order to be ready to receive commands.
    pub fn wake(&mut self) {
        self.write_bytes(&[ESC, 0x38, 0x00, 0x00]);
        self.sleep(75_000);
    }

    /// Block for 500ms to allow the printer to boot, then wake it up, disable sleep, and call reset.
    pub fn init(&mut self) {
        // Allow time for the printer to initialize
        self.sleep(500_000);
        self.wake();
        // Disable sleep
        self.write_bytes(&[ESC, 0x38, 0x00, 0x00]);
        self.reset();
        self.feed();
    }

    /// Update internal representations of char height and width depending on the configured font
    /// and print modes.
    fn adjust_char_values(&mut self, print_mode: PrintMode) {
        // Check font
        self.char_height = match print_mode.font {
            Font::FontA => 24,
            Font::FontB => 17,
        };
        self.char_width = match print_mode.font {
            Font::FontA => 12,
            Font::FontB => 9,
        };

        // Check for double-width mode
        if print_mode.double_width {
            self.char_width *= 2;
        }

        // Check for double-height mode
        if print_mode.double_height {
            self.char_height *= 2;
        }

        self.max_column = (DOT_WIDTH / self.char_width as u32) as u8;
    }

    /// Select print mode(s), such as inverse printing or double-height mode.
    pub fn set_print_mode(&mut self, print_mode: PrintMode) {
        let mode_byte: u8 = print_mode.into();
        self.print_mode = mode_byte;

        self.write_bytes(&MODE_SEQUENCE);
        self.write_byte(mode_byte).unwrap();

        // For some modes a custom command seems to be necessary
        let modes = [
            print_mode.inverse as u8,
            print_mode.upside_down as u8,
            print_mode.emph as u8,
        ];
        for pair in zip(MODE_ORDER, modes) {
            let (cmd, n) = pair;
            self.write_bytes(&cmd);
            self.write_byte(n).unwrap();
        }

        self.adjust_char_values(print_mode);
    }

    /// Configure print settings. See [`PrintSettings`] for more information.
    pub fn set_print_settings(&mut self, print_settings: PrintSettings) {
        let settings_bytes: [u8; 3] = print_settings.into();
        self.write_bytes(&[ESC, 0x37]);
        self.write_bytes(&settings_bytes);
    }

    pub fn set_justification(&mut self, justification: Justification) {
        let justification_byte = match justification {
            Justification::Left => 0x00,
            Justification::Center => 0x01,
            Justification::Right => 0x02,
        };
        self.write_bytes(&[ESC, 0x61, justification_byte]);
    }

    pub fn set_underline(&mut self, mode: Underline) {
        let underline_byte = match mode {
            Underline::None => 0x00,
            Underline::Normal => 0x01,
            Underline::Double => 0x02,
        };
        self.write_bytes(&[ESC, 0x2D, underline_byte]);
    }

    pub fn set_character_set(&mut self, character_set: CharacterSet) {
        self.write_bytes(&[ESC, 0x52, character_set.into()]);
    }

    pub fn set_code_table(&mut self, code_table: CodeTable) {
        self.write_bytes(&[ESC, 0x74, code_table.into()]);
    }

    /// Print a bitmap image. This command is not affected by print modes, but justification is
    /// respected.
    ///
    /// Since only monochrome images can be printed, the raw color value of each pixel is matched
    /// against [`PIXEL_COLOR_CUTOFF`]. If a pixel color is below this value, it produces a dot in
    /// the output (reasoning that darker pixels should be printed, while lighter ones should not
    /// be), otherwise not.
    ///
    /// # Example
    /// ```
    /// printer.init();
    /// printer.print_bitmap(
    ///     RawBmp::from_slice(
    ///         include_bytes!("../resources/ferris.bmp")
    ///     ).unwrap(),
    ///     RasterBitImageMode::Normal
    /// );
    /// ```
    pub fn print_bitmap(&mut self, bmp: RawBmp, mode: RasterBitImageMode) {
        let x_bits = bmp.header().image_size.width;
        let x_bytes = (x_bits / 8) as u8 + u8::from(x_bits % 8 != 0);
        let y_bits = bmp.header().image_size.height;

        // I don't understand what xH and yH are, but setting them to 0 seems to work.
        self.write_bytes(&[GS, 0x76, 0, mode.into(), x_bytes, 0, y_bits as u8, 0]);

        let mut image_bits = bitvec![u8, Msb0;];
        for pixel in bmp.pixels() {
            let column = pixel.position.x as u32;

            image_bits.push(pixel.color < PIXEL_COLOR_CUTOFF);

            if column == ((x_bits - 1) as u32) && x_bits % 8 > 0 {
                let fill_bits = 8 - (x_bits % 8);

                for _ in 0..fill_bits {
                    image_bits.push(false);
                }
            }
        }
        for (i, byte) in image_bits.as_raw_slice().iter().enumerate() {
            self.write_byte(*byte).unwrap();
            if i as u8 % x_bytes == 0 {
                self.sleep((self.dot_print_time + self.dot_feed_time) as u64);
            }
        }
    }

    /// Print a barcode with the specified `BarCodeSystem`. Note that each system requires a
    /// specific range of characters.
    pub fn print_barcode(&mut self, system: BarCodeSystem, text: &str) {
        self.write_bytes(&[GS, 0x6B, system.into(), text.len() as u8]);
        for b in text.chars() {
            self.write_byte(b as u8).unwrap();
        }
        self.sleep(self.barcode_height as u64 * (self.dot_print_time + self.dot_feed_time) as u64)
    }

    /// Set the barcode height to the specified number of dots.
    pub fn set_barcode_height(&mut self, height: u8) {
        self.write_bytes(&[GS, 0x68, height]);
    }

    /// Set the space to the left of the barcode to the specified number of dots.
    pub fn set_barcode_left_space(&mut self, space: u8) {
        self.write_bytes(&[GS, 0x78, space]);
    }

    /// Set the barcode width to the specified value. See [`BarcodeWidth`] for more information.
    pub fn set_barcode_width(&mut self, width: BarcodeWidth) {
        self.write_bytes(&[GS, 0x77, width.into()]);
    }

    /// Feed the paper by exactly one line.
    pub fn feed(&mut self) {
        self.feed_n(1);
    }

    /// Feed the paper by the specified number of lines.
    pub fn feed_n(&mut self, lines: u8) {
        self.write_bytes(&[ESC, 0x4A, lines]);

        let dot_feed_time: u64 = self.dot_feed_time.into();
        let char_height: u64 = self.char_height.into();
        self.sleep(dot_feed_time * char_height);
        self.prev_byte = '\n';
        self.current_column = 0;
    }
}

impl<Port: serial::Write<u8>, Delay: delay::DelayUs<u32>> Write for Printer<Port, Delay> {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        self.write(s.as_bytes());
        Ok(())
    }

    fn write_char(&mut self, c: char) -> Result<(), Error> {
        self.write_one(c as u8).unwrap();
        Ok(())
    }

    fn write_fmt(&mut self, args: Arguments<'_>) -> Result<(), Error> {
        self.write_str(format!("{}", args).as_str())
    }
}
