pub mod wcet;

use std::fs::OpenOptions;
use std::io::{BufWriter, Read, Write};
use std::os::unix::fs::MetadataExt;

use clap::Parser;
use elf::ElfBytes;
use elf::endian::AnyEndian;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short)]
    file: String,

    #[arg(short)]
    symbol: String,

    #[arg(long)]
    flash: bool,
    #[arg(long)]
    device: Option<String>,
    #[arg(short, long, default_value_t = 115200)]
    baudrate: u32,
}

fn main() {
    let args = Args::parse();

    let path = std::path::PathBuf::from(args.file.clone());
    let file_data = std::fs::read(path).expect("failed to read the elf file");
    let slice = file_data.as_slice();
    let file = ElfBytes::<AnyEndian>::minimal_parse(slice).expect("failed to parse the elf file");

    let dump_path = std::path::PathBuf::from("elf.dump");
    let dump_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dump_path)
        .expect("failed to create a dump file");

    let mut buf_writer = BufWriter::new(&dump_file);

    // LOADPROG magic
    buf_writer.write_all("LOADPROG".as_bytes()).unwrap();

    let header = file.ehdr;
    println!("type: {}", header.e_type);
    println!("machine: {}", header.e_machine);
    println!("entry: 0x{:X}", header.e_entry);

    // writing size
    let metadata = std::fs::metadata(args.file.clone()).unwrap();
    let size = metadata.size().to_le_bytes();
    buf_writer.write_all(&size[0..8]).unwrap();

    println!(
        "file size in bytes: {:?} => {:?}",
        metadata.size(),
        &size[0..8]
    );

    let symbol_table = file.symbol_table().unwrap().unwrap();
    let string_table = file.symbol_table().unwrap().unwrap().1;
    // for symbol in symbol_table.0 {
    //     let data = symbol_table.1.get(symbol.st_name as usize).unwrap();
    //     if data == args.symbol {
    //         println!("symbol at {:?}", symbol.st_value);
    //         let value = &symbol.st_value.to_le_bytes();
    //         buf_writer.write_all(&value[0..8]).unwrap();
    //         println!(
    //             "symbol address {:?} (0x{:X}) => {:?}",
    //             symbol.st_value,
    //             symbol.st_value,
    //             &value[0..8]
    //         );
    //         break;
    //     }
    // }

    // let sym_table_clone = symbol_table.clone();
    // let symbol_iterator = symbol_table.0.clone();
    let symbols = symbol_table
        .0
        .into_iter()
        .find(|s| args.symbol == string_table.get(s.st_name as usize).unwrap())
        .expect(&format!("symbol with name {} not found", args.symbol));

    // symbol_table_iterator
    //     .into_iter()
    //     .for_each(|symbol| {
    //         let name = string_table.get(symbol.st_name as usize).unwrap();
    //         println!("Symbol {} => size: {}, value: 0x{:X}", name, symbol.st_size, symbol.st_value);
    //     });

    let symbol_addr = symbols.st_value;
    let symbol_addr_bytes = &symbol_addr.to_le_bytes()[0..8];
    println!(
        "symbol address {:?} (0x{:X}) => {:?}",
        symbol_addr, symbol_addr, symbol_addr_bytes
    );

    buf_writer.write_all(symbol_addr_bytes).unwrap();

    buf_writer
        .write_all(file_data.as_slice())
        .expect("failed to write the elf file");
    buf_writer.flush().expect("failed to flush file");

    println!("Written {} bytes to a file", file_data.len());

    if !args.flash {
        return;
    }

    if args.device.is_none() {
        println!("Cant flash as no device provided");
        return;
    }

    let mut serial = serialport::new(args.device.as_ref().unwrap(), args.baudrate)
        .open()
        .unwrap();
    let mut content = Vec::new();
    std::fs::File::open("elf.dump")
        .unwrap()
        .read_to_end(&mut content)
        .unwrap();

    println!("Preparing to write {} bytes", content.len());
    for byte in content {
        serial.write(&[byte]).unwrap();
        serial.flush().unwrap();
    }

    serial.flush().unwrap();
}
