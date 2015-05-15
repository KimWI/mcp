//
// MSX CAS Packager
// Copyright (c) 2015 Alvaro Polo
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::io::Read;
use std::io::Error as IoError;
use std::io::Write;
use std::str::from_utf8;

/// A block of data contained in a tape.
///
/// A tape file is comprised by a sequence of blocks. Each block starts with the prefix bytes
/// `1fa6debacc137d74` followed by the block data. The `Block` type stores the block data
/// including the prefix bytes.
///
#[derive(Debug)]
pub struct Block {
    data: Vec<u8>,
}

impl Block {

    /// Generates a new block from the data bytes (without the prefix bytes).
    pub fn from_data(bytes: &[u8]) -> Block {
        let mut data = Vec::with_capacity(bytes.len() + 8);
        data.write(&[0x1f, 0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74]).unwrap();
        data.write(bytes).unwrap();
        Block { data: data }
    }

    /// Returns the block data (including the prefix bytes).
    pub fn data(&self) -> &[u8] { &self.data[..] }

    /// Returns the block data (without the prefix bytes).
    pub fn data_without_prefix(&self) -> &[u8] { &self.data[8..] }

    /// Returns `true` if the block is detected as a binary header.
    ///
    /// A bin header is comprised by `0xd0d0d0d0d0d0d0d0d0d0` followed by six bytes for
    /// the name of the binary file. This function returns `true` if the block data match
    /// this pattern, `false` otherwise.
    pub fn is_bin_header(&self) -> bool {
        let data = self.data_without_prefix();
        data.len() == 16 &&
            data[..10] == [0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0]
    }

    /// Returns `true` if the block is detected as a Basic header.
    ///
    /// A Basic header is comprised by `0xd3d3d3d3d3d3d3d3d3d3` followed by six bytes for
    /// the name of the Basic file. This function returns `true` if the block data match
    /// this pattern, `false` otherwise.
    pub fn is_basic_header(&self) -> bool {
        let data = self.data_without_prefix();
        data.len() == 16 &&
            data[..10] == [0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3]
    }

    /// Returns `true` if the block is detected as an ASCII header.
    ///
    /// An ASCII header is comprised by `0xeaeaeaeaeaeaeaeaeaea` followed by six bytes for
    /// the name of the ASCII file. This function returns `true` if the block data match
    /// this pattern, `false` otherwise.
    pub fn is_ascii_header(&self) -> bool {
        let data = self.data_without_prefix();
        data.len() == 16 &&
            data[..10] == [0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea]
    }

    /// Returns the file name in case of a binary, ascii or basic header, `None` otherwise.
    pub fn file_name(&self) -> Option<&str> {
        if self.is_bin_header() || self.is_basic_header() || self.is_ascii_header() {
            let name = &self.data_without_prefix()[10..16];
            let whites: &[_] = &['\0', ' '];
            from_utf8(name)
                .ok()
                .map(|n| n.trim_right_matches(whites))
        }
        else { None }
    }
}

/// A file contained in a tape
///
/// Files stored in a tape can be one of:
/// * Binary files. They contain binary code and/or data. They are loaded using `BLOAD`
///   instruction from Basic.
/// * ASCII files. They contain text, typically corresponding to Basic source code. They are
///   loaded using `LOAD` instruction from Basic.
/// * Basic files. They contain tokenized Basic, a compact form of Basic source code. They are
///   loaded using `CLOAD` instruction from Basic.
/// * Custom files. They contain arbitrary data generated by a program using direct calls to
///   casette IO addresses. Its contents cannot be processed from Basic but loaded from the
///   program that generates them in a custom way.
///
/// `File` instances are generated in iteration from `files()` function of `Tape` type.
///
#[derive(Debug, PartialEq)]
pub enum File<'a> {
	Bin(String, usize, usize, usize, &'a[u8]),
    Basic(String, &'a[u8]),
    Ascii(String, Vec<&'a[u8]>),
    Custom(&'a[u8])
}

impl<'a> File<'a> {

    /// Returns the name of this file, or `None` if it has no name.
    pub fn name(&self) -> Option<String> {
        match self {
            &File::Bin(ref name, _, _, _, _) => Some(format!("{}.bin", name)),
            &File::Basic(ref name, _) => Some(format!("{}.bas", name)),
            &File::Ascii(ref name, _) => Some(format!("{}.asc", name)),
            _ => None,
        }
    }
}

/// An iterator over the files of a `Tape`
pub struct Files<'a> {
    tape: &'a Tape,
    i: usize,
}

impl<'a> Iterator for Files<'a> {

    type Item = File<'a>;

    fn next(&mut self) -> Option<File<'a>> {
        let nblocks = self.tape.blocks.len();
        while self.i < nblocks {
            let block = &self.tape.blocks[self.i];
            if block.is_bin_header() {
                let name = block.file_name().unwrap().to_string();
                let content = &self.tape.blocks[self.i+1].data_without_prefix();
                let begin = (content[0] as usize) | (content[1] as usize) << 8;
                let end = (content[2] as usize) | (content[3] as usize) << 8;
                let start = (content[4] as usize) | (content[5] as usize) << 8;
                let data = &content[..];
                self.i += 2;
                return Some(File::Bin(name, begin, end, start, data));
            } else if block.is_basic_header() {
                let name = block.file_name().unwrap().to_string();
                let content = &self.tape.blocks[self.i+1].data_without_prefix();
                self.i += 2;
                return Some(File::Basic(name, &content[..]));
            } else if block.is_ascii_header() {
                let name = block.file_name().unwrap().to_string();
                let mut data = Vec::<&[u8]>::new();
                self.i += 1;
                while {
                    let chunk = &self.tape.blocks[self.i].data_without_prefix();
                    data.push(chunk);
                    self.i < nblocks && !chunk.contains(&0x1a)
                } { self.i += 1 }
                self.i += 1;
                return Some(File::Ascii(name, data));
            } else {
                self.i += 1;
                return Some(File::Custom(&block.data[..]));
            }
        }
        None
    }
}

/// An MSX tape.
///
/// A tape is a sequence of byte blocks (see `Blocks` for more details). The blocks may be
/// grouped such as the tape is seen as a sequence of files through `files()` method.
///
#[derive(Debug)]
pub struct Tape {
    blocks: Vec<Block>,
}

impl Tape {

    /// Create a new empty tape.
    pub fn new() -> Tape { Tape { blocks: vec![] }}

    /// Read a `Tape` instance from the given `Read` object.
    ///
    /// This function returns a new `Tape` instance as result of processing the
    /// contents of the `Read` passed as argument (e.g., a file), or an `std::io::Error`
    /// if there is an error while reading.
    ///
	#[allow(dead_code)]
	pub fn read<R: Read>(input: &mut R) -> Result<Tape, IoError> {
		let mut bytes: Vec<u8> = vec![];
		try!(input.read_to_end(&mut bytes));
		Ok(Tape::from_bytes(&bytes[..]))
	}

    /// Read a `Tape` instance from the given bytes.
    ///
    /// This function returns a new `Tape` instance as result of processing the bytes passed
    /// as argument.
	pub fn from_bytes(bytes: &[u8]) -> Tape {
		Tape { blocks: Tape::parse_blocks(bytes) }
	}

    /// Returns the blocks of this tape.
    pub fn blocks(&self) -> &[Block] { &self.blocks[..] }

    /// Return the files contained in the tape.
    ///
    /// This function returns an `Iterator` over the files found in the tape blocks.
    ///
    pub fn files(&self) -> Files { Files { tape: self, i: 0 } }

    /// Append a binary file to this tape
    ///
    /// This method appends a binary file to the tape by generating the corresponding
    /// header & data blocks for the file from the following arguments:
    ///
    /// * `name`: the six bytes that conforms the file name. Use function `file_name()` to
    ///   obtain it from a regular string.
    /// * `data`: the binary file content
    ///
    pub fn append_bin(&mut self, name: &[u8;6], data: &[u8]) {
        let hblock = Block::from_data(&[
            0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0,
            name[0], name[1], name[2], name[3], name[4], name[5],
        ]);
        let dblock = Block::from_data(data);
        self.blocks.push(hblock);
        self.blocks.push(dblock);
    }

    /// Append a binary file to this tape
    ///
    /// This method appends a binary file to the tape by generating the corresponding
    /// header & data blocks for the file from the following arguments:
    ///
    /// * `name`: the six bytes that conforms the file name. Use function `file_name()` to
    ///   obtain it from a regular string.
    /// * `data`: the binary file content
    ///
    pub fn append_basic(&mut self, name: &[u8;6], data: &[u8]) {
        let hblock = Block::from_data(&[
            0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3,
            name[0], name[1], name[2], name[3], name[4], name[5],
        ]);
        let dblock = Block::from_data(data);
        self.blocks.push(hblock);
        self.blocks.push(dblock);
    }

    /// Append an ASCII file to this tape
    ///
    /// This method appends an ASCII file to the tape by generating the corresponding
    /// header & data blocks for the file from the following arguments:
    ///
    /// * `name`: the six bytes that conforms the file name. Use function `file_name()` to
    ///   obtain it from a regular string.
    /// * `data`: the binary file content
    ///
    /// The ASCII files are stored in a very particular manner in CAS format. The whole
    /// text is divided in chunks of 256 bytes. The last block must end with at least one
    /// EOF byte. As result, the last block is padded with EOFs until it occupies 256 bytes.
    /// If the text length is a multiple of 256, the last block is 256 EOF bytes.
    ///
    pub fn append_ascii(&mut self, name: &[u8;6], data: &[u8]) {
        let hblock = Block::from_data(&[
            0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea,
            name[0], name[1], name[2], name[3], name[4], name[5],
        ]);
        self.blocks.push(hblock);
        for chunk in data.chunks(256) {
            let dblock = Block::from_data(chunk);
            self.blocks.push(dblock);
        }
        if data.len() % 256 == 0 {
            // We need another block for the EOFs
            let eofs: [u8; 256] = [0x1a; 256];
            self.blocks.push(Block::from_data(&eofs));
        } else {
            let last_block = self.blocks.last_mut().unwrap();
            while last_block.data_without_prefix().len() < 256 {
                last_block.data.push(0x1a);
            }
        }
    }

    fn parse_blocks(bytes: &[u8]) -> Vec<Block> {
        let mut blocks: Vec<Block> = vec![];
        let mut hindex: Vec<usize> = vec![];
        let mut i = 0;

        // First of all, we compute the indices of all block headers.
        for win in bytes.windows(8) {
            if win == [0x1f, 0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74] {
                hindex.push(i);
            }
            i = i + 1;
        }

        // Now we use the block header indices to generate the blocks
        for i in 0..hindex.len() {
            let from = hindex[i] + 8;
            let to = if i == hindex.len() - 1 { bytes.len() } else { hindex[i+1] };
            let block = Block::from_data(&bytes[from..to]);
            blocks.push(block);
        }
        blocks
    }
}

/// Converts a string into a tape filename
///
/// This function converts the string passed as argument into a tape file name.
/// The tape filename is comprised by six ASCII characters. If the given string
/// is too long, it is truncated.
///
pub fn file_name(s: &str) -> [u8; 6] {
    use std::cmp::min;

    let last = if s.len() > 6 { 6 } else { s.len() };

    let mut name: [u8; 6] = [0x20; 6];
    let bytes = &s.as_bytes()[..last];
    for i in 0..min(6, s.len()) { name[i] = bytes[i] }
    name
}

#[cfg(test)]
mod test {

    use std::iter::FromIterator;

    use super::*;

    macro_rules! assert_bin {
        ($f:expr, $n:expr, $b:expr, $e:expr, $s:expr, $d:expr) => {
            match $f {
            &File::Bin(ref name, begin, end, start, data) => {
                assert_eq!($n, name);
                assert_eq!($b, begin);
                assert_eq!($e, end);
                assert_eq!($s, start);
                assert_eq!($d, &data[6..]);
            },
            _ => panic!("unexpected file"),
        }
        }
    }

    macro_rules! assert_ascii {
        ($f:expr, $n:expr, $d:expr) => {
            match $f {
            &File::Ascii(ref name, ref data) => {
                assert_eq!($n, name);
                assert_eq!($d, &data[..]);
            },
            _ => panic!("unexpected file"),
        }
        }
    }

    #[test]
    fn should_detect_bin_header_block() {
        let bytes: Vec<u8> = vec![
            0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0,
            0xd0, 0xd0, 0x46, 0x4f, 0x4f, 0x42, 0x41, 0x52,
        ];
        let block = Block::from_data(&bytes);
        assert!(block.is_bin_header());
        assert_eq!("FOOBAR", block.file_name().unwrap());
    }

    #[test]
    fn should_detect_basic_header_block() {
        let bytes: Vec<u8> = vec![
            0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3, 0xd3,
            0xd3, 0xd3, 0x46, 0x4f, 0x4f, 0x42, 0x41, 0x52,
        ];
        let block = Block::from_data(&bytes);
        assert!(block.is_basic_header());
        assert_eq!("FOOBAR", block.file_name().unwrap());
    }

    #[test]
    fn should_detect_ascii_header_block() {
        let bytes: Vec<u8> = vec![
            0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea,
            0xea, 0xea, 0x46, 0x4f, 0x4f, 0x42, 0x41, 0x52,
        ];
        let block = Block::from_data(&bytes);
        assert!(block.is_ascii_header());
        assert_eq!("FOOBAR", block.file_name().unwrap());
    }

    #[test]
    fn should_return_no_name_on_non_header_block() {
        let bytes: Vec<u8> = vec![
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x46, 0x4f, 0x4f, 0x00, 0x00, 0x00,
        ];
        let block = Block::from_data(&bytes);
        assert_eq!(None, block.file_name());
    }

    #[test]
    fn should_detect_block_with_short_name() {
        let bytes: Vec<u8> = vec![
            0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0,
            0xd0, 0xd0, 0x46, 0x4f, 0x4f, 0x20, 0x20, 0x20,
        ];
        let block = Block::from_data(&bytes);
        assert_eq!("FOO", block.file_name().unwrap());
    }

    #[test]
    fn should_load_empty_tape() {
    	let bytes: Vec<u8> = vec![];
    	let tape = Tape::from_bytes(&bytes);
        assert_eq!(None, tape.files().next());
    }

    #[test]
    fn should_load_tape_with_some_blocks() {
        let bytes: Vec<u8> = vec![
            0x1f, 0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74,
            0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0,
            0xd0, 0xd0, 0x46, 0x49, 0x4c, 0x45, 0x31, 0x00,
            0x1f, 0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74,
            0x00, 0x80, 0x08, 0x80, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x1f,
            0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74, 0xea,
            0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea,
            0xea, 0x46, 0x49, 0x4c, 0x45, 0x32, 0x00, 0x1f,
            0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74, 0x41,
            0x42, 0x43, 0x44, 0x1a,
        ];
        let tape = Tape::from_bytes(&bytes);
        assert_eq!(4, tape.blocks().len());

        assert!(tape.blocks()[0].is_bin_header());
        assert_eq!("FILE1", tape.blocks()[0].file_name().unwrap());

        assert!(tape.blocks()[2].is_ascii_header());
        assert_eq!("FILE2", tape.blocks()[2].file_name().unwrap());
    }

    #[test]
    fn should_load_tape_with_some_files() {
        let bytes: Vec<u8> = vec![
            0x1f, 0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74,
            0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0, 0xd0,
            0xd0, 0xd0, 0x46, 0x49, 0x4c, 0x45, 0x31, 0x20,
            0x1f, 0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74,
            0x00, 0x80, 0x08, 0x80, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x1f,
            0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74, 0xea,
            0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea, 0xea,
            0xea, 0x46, 0x49, 0x4c, 0x45, 0x32, 0x20, 0x1f,
            0xa6, 0xde, 0xba, 0xcc, 0x13, 0x7d, 0x74, 0x41,
            0x42, 0x43, 0x44, 0x1f, 0xa6, 0xde, 0xba, 0xcc,
            0x13, 0x7d, 0x74, 0x45, 0x46, 0x47, 0x1a,
        ];
        let tape = Tape::from_bytes(&bytes);
        let files = Vec::from_iter(tape.files());
        assert_eq!(2, files.len());

        assert_eq!("FILE1.bin", files[0].name().unwrap());
        assert_bin!(&files[0],
            "FILE1", 0x8000, 0x8008, 0x0000,
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09]);

        assert_eq!("FILE2.asc", files[1].name().unwrap());
        assert_ascii!(&files[1],
            "FILE2",
            vec![&[0x41, 0x42, 0x43, 0x44 ],
                 &[0x45, 0x46, 0x47, 0x1a ]]);
    }

    #[test]
    fn should_add_bin_file() {
        let mut tape = Tape::new();
        let fname = file_name(&"foobar");
        let data = [ 0x00, 0x80, 0x04, 0x80, 0x00, 0x80, 0x00, 0x01, 0x02, 0x03 ];
        tape.append_bin(&fname, &data);

        let files = Vec::from_iter(tape.files());
        assert_eq!(1, files.len());
        assert_eq!("foobar.bin", files[0].name().unwrap());
        assert_bin!(&files[0],
            "foobar", 0x8000, 0x8004, 0x8000, &[0x00, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn should_add_basic_file() {
        let mut tape = Tape::new();
        let fname = file_name(&"foobar");
        let data = [ 0x00, 0x80, 0x04, 0x80, 0x00, 0x80, 0x00, 0x01, 0x02, 0x03 ];
        tape.append_basic(&fname, &data);

        let blocks = tape.blocks();
        let files = Vec::from_iter(tape.files());

        assert_eq!(2, blocks.len());
        assert_eq!(data, blocks[1].data_without_prefix());

        assert_eq!(1, files.len());
        assert_eq!("foobar.bas", files[0].name().unwrap());
    }

    #[test]
    fn should_add_ascii_file() {
        let mut tape = Tape::new();
        let fname = file_name(&"foobar");
        let data: [u8; 500] = [ 'A' as u8; 500];
        tape.append_ascii(&fname, &data);

        let blocks = tape.blocks();
        let files = Vec::from_iter(tape.files());

        assert_eq!(3, blocks.len());
        assert_eq!(256, blocks[1].data_without_prefix().len());
        assert_eq!(256, blocks[2].data_without_prefix().len());
        let eofs_found = blocks[2].data_without_prefix().iter().filter(|b| **b == 0x1a).count();
        assert_eq!(12, eofs_found);

        assert_eq!(1, files.len());
        assert_eq!("foobar.asc", files[0].name().unwrap());
    }

    #[test]
    fn should_add_ascii_file_aligned_256() {
        let mut tape = Tape::new();
        let fname = file_name(&"foobar");
        let data: [u8; 512] = [ 'A' as u8; 512];
        tape.append_ascii(&fname, &data);

        let blocks = tape.blocks();
        let files = Vec::from_iter(tape.files());

        assert_eq!(4, blocks.len());
        assert_eq!(256, blocks[1].data_without_prefix().len());
        assert_eq!(256, blocks[2].data_without_prefix().len());
        assert_eq!(256, blocks[3].data_without_prefix().len());
        let eofs_found = blocks[3].data_without_prefix().iter().filter(|b| **b == 0x1a).count();
        assert_eq!(256, eofs_found);

        assert_eq!(1, files.len());
        assert_eq!("foobar.asc", files[0].name().unwrap());
    }
}
