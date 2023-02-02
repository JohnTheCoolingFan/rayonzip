use flate2::{read::DeflateEncoder, Compression, CrcReader};
use rayon::{prelude::*, ThreadPool};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::Path,
    sync::mpsc::{channel, Receiver, Sender},
};

const VERSION_NEEDED_TO_EXTRACT: u16 = 20;
const VERSION_MADE_BY: u16 = 0x033F;

const FILE_RECORD_SIGNATURE: u32 = 0x04034B50;
const DIRECTORY_ENTRY_SIGNATURE: u32 = 0x02014B50;
const END_OF_CENTRAL_DIR_SIGNATURE: u32 = 0x06054B50;

/// Making archives with stored compression is not supported yet and only used on directory
/// entries.
#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum CompressionType {
    Stored = 0,
    Deflate = 8,
}

#[derive(Debug)]
pub struct ZipArchive<'a> {
    thread_pool: &'a ThreadPool,
    tx: Sender<ZipFile>,
    rx: Receiver<ZipFile>,
}

impl<'a> ZipArchive<'a> {
    pub fn new(thread_pool: &'a ThreadPool) -> Self {
        let (tx, rx) = channel();
        Self {
            thread_pool,
            tx,
            rx,
        }
    }

    fn fs_file_to_archive_file(fs_path: &Path, archived_name: &str) -> ZipFile {
        let file = File::open(fs_path).unwrap();
        let uncompressed_size = file.metadata().unwrap().len() as u32;
        let crc_reader = CrcReader::new(file);
        let mut encoder = DeflateEncoder::new(crc_reader, Compression::new(9));
        let mut data = Vec::new();
        encoder.read_to_end(&mut data).unwrap();
        let crc_reader = encoder.into_inner();
        let crc = crc_reader.crc().sum();
        ZipFile {
            compression_type: CompressionType::Deflate,
            crc,
            uncompressed_size,
            filename: archived_name.into(),
            data,
            external_file_attributes: 0o100644 << 16, // Possible improvement: read
                                                      // permissions/attributes from fs
        }
    }

    fn slice_to_archive_file(slice: &[u8], archived_name: &str) -> ZipFile {
        let uncompressed_size = slice.len() as u32;
        let crc_reader = CrcReader::new(slice);
        let mut encoder = DeflateEncoder::new(crc_reader, Compression::new(9));
        let mut data = Vec::new();
        encoder.read_to_end(&mut data).unwrap();
        let crc_reader = encoder.into_inner();
        let crc = crc_reader.crc().sum();
        ZipFile {
            compression_type: CompressionType::Deflate,
            crc,
            uncompressed_size,
            filename: archived_name.into(),
            data,
            external_file_attributes: 0o100644 << 16,
        }
    }

    pub fn add_file_from_fs(&mut self, fs_path: &Path, archived_name: &str) {
        let thread_tx = self.tx.clone();
        let fs_path = fs_path.to_path_buf();
        let archived_name = archived_name.to_string();
        self.thread_pool.spawn(move || {
            thread_tx
                .send(Self::fs_file_to_archive_file(&fs_path, &archived_name))
                .unwrap()
        })
    }

    pub fn add_file_from_slice(&mut self, slice: &[u8], archived_name: &str) {
        let thread_tx = self.tx.clone();
        let slice = slice.to_vec();
        let archived_name = archived_name.to_string();
        self.thread_pool.spawn(move || {
            thread_tx
                .send(Self::slice_to_archive_file(&slice, &archived_name))
                .unwrap()
        })
    }

    pub fn add_directory(&mut self, archived_name: &str) {
        let compressed_file = ZipFile::directory(archived_name.into());
        self.tx.send(compressed_file).unwrap();
    }

    pub fn write<W: Write + Seek>(self, destination: &mut W) -> Result<(), std::io::Error> {
        let Self {
            thread_pool,
            tx,
            rx,
        } = self;
        drop(tx);

        let files: Vec<ZipFile> = thread_pool.install(|| rx.into_iter().par_bridge().collect());

        let mut offsets = Vec::new();
        for file in &files {
            offsets.push(destination.stream_position().unwrap() as u32);
            file.to_bytes_filerecord(destination);
        }
        let central_dir_offset = destination.stream_position()? as u32;
        for (file, offset) in files.iter().zip(offsets.into_iter()) {
            file.to_bytes_direntry(destination, offset)
        }
        let central_dir_start = destination.stream_position()? as u32;

        // Signature
        destination
            .write_all(&END_OF_CENTRAL_DIR_SIGNATURE.to_le_bytes())
            .unwrap();
        // number of this disk
        destination.write_all(&0_u16.to_le_bytes()).unwrap();
        // number of the disk with start
        destination.write_all(&0_u16.to_le_bytes()).unwrap();
        // Number of entries on this disk
        destination.write_all(&files.len().to_le_bytes()).unwrap();
        // Number of entries
        destination.write_all(&files.len().to_le_bytes()).unwrap();
        // Central dir size
        destination
            .write_all(&(central_dir_start - central_dir_offset).to_le_bytes())
            .unwrap();
        // Central dir offset
        destination
            .write_all(&central_dir_offset.to_le_bytes())
            .unwrap();
        // Comment length
        destination.write_all(&0_u16.to_le_bytes()).unwrap();

        Ok(())
    }
}

#[derive(Debug)]
struct ZipFile {
    compression_type: CompressionType,
    crc: u32,
    uncompressed_size: u32,
    filename: String,
    data: Vec<u8>,
    external_file_attributes: u32,
}

impl ZipFile {
    fn to_bytes_filerecord<W: Write + Seek>(&self, buf: &mut W) {
        // signature
        buf.write_all(&FILE_RECORD_SIGNATURE.to_le_bytes()).unwrap();
        // version needed to extract
        buf.write_all(&VERSION_NEEDED_TO_EXTRACT.to_le_bytes())
            .unwrap();
        // flags
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // compression type
        buf.write_all(&(self.compression_type as u16).to_le_bytes())
            .unwrap();
        // Time // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Date // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // crc
        buf.write_all(&self.crc.to_le_bytes()).unwrap();
        // Compressed size
        buf.write_all(&(self.data.len() as u32).to_le_bytes())
            .unwrap();
        // Uncompressed size
        buf.write_all(&self.uncompressed_size.to_le_bytes())
            .unwrap();
        // Filename size
        buf.write_all(&(self.filename.len() as u16).to_le_bytes())
            .unwrap();
        // extra field size
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Filename
        buf.write_all(self.filename.as_bytes()).unwrap();
        // Data
        buf.write_all(&self.data).unwrap();
    }

    fn to_bytes_direntry<W: Write + Seek>(&self, buf: &mut W, local_header_offset: u32) {
        // signature
        buf.write_all(&DIRECTORY_ENTRY_SIGNATURE.to_le_bytes())
            .unwrap();
        // version made by
        buf.write_all(&VERSION_MADE_BY.to_le_bytes()).unwrap();
        // version needed to extract
        buf.write_all(&VERSION_NEEDED_TO_EXTRACT.to_le_bytes())
            .unwrap();
        // flags
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // compression type
        buf.write_all(&(self.compression_type as u16).to_le_bytes())
            .unwrap();
        // Time // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Date // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // crc
        buf.write_all(&self.crc.to_le_bytes()).unwrap();
        // Compressed size
        buf.write_all(&(self.data.len() as u32).to_le_bytes())
            .unwrap();
        // Uncompressed size
        buf.write_all(&self.uncompressed_size.to_le_bytes())
            .unwrap();
        // Filename size
        buf.write_all(&(self.filename.len() as u16).to_le_bytes())
            .unwrap();
        // extra field size
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // comment size
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // disk number start
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // internal file attributes
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // external file attributes
        buf.write_all(&self.external_file_attributes.to_le_bytes())
            .unwrap();
        // relative offset of local header
        buf.write_all(&local_header_offset.to_le_bytes()).unwrap();
        // Filename
        buf.write_all(self.filename.as_bytes()).unwrap();
    }

    fn directory(mut name: String) -> Self {
        name = name.replace('\\', "/");
        if !(name.ends_with('/') || name.ends_with('\\')) {
            name += "/"
        };
        Self {
            compression_type: CompressionType::Stored,
            crc: 0,
            uncompressed_size: 0,
            filename: name,
            data: vec![],
            external_file_attributes: 0o40755 << 16,
        }
    }
}
