use std::fs::OpenOptions;
use std::io::{Read, Write, stdout};
use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep};
use std::time;

use clap::Parser;
use clap::Subcommand;
use elf::ElfBytes;
use elf::endian::AnyEndian;
use serialport::SerialPort;

const LOAD_PROG_MAGIC: &[u8] = "LOADPROG".as_bytes();
const KILL_PROG_MAGIC: &[u8] = "KILLTASK".as_bytes();
const LIST_TASKS_MAGIC: &[u8] = "LISTPROG".as_bytes();
const RELOAD_TASKS_MAGIC: &[u8] = "RELAUNCH".as_bytes();

/// Serial Interface for the PicOS project
/// (gh: https://github.com/pes-innovation-lab/possum)
#[derive(Parser, Debug)]
struct Args {
    /// The directives for the program
    #[command(subcommand)]
    cmd: Commands,

    /// The /dev device for the Pico
    #[arg(short, long)]
    device: String,
    /// Baudrate for UART
    #[arg(short, long, default_value_t = 115200)]
    baudrate: u32,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Load a program built with PIC to the Pico
    Load {
        file: String,
        /// symbol for the entry point of the program
        symbol: String,
        /// 8 byte identifier
        identifier: String,
    },
    /// Kill a task on the pico
    Kill {
        /// The id of the taks to be killed
        identifier: String,
    },
    /// Relaunch a task on the Pico
    Relaunch {
        /// The id of the taks to be killed
        identifier: String,
    },
    /// List all tasks and status on the Pico
    List,
    /// Serial logger
    Log,
}

fn handle_load_cmd(file: String, symbol: String, identifier: String) -> Option<Vec<u8>> {
    let path = std::path::PathBuf::from(file.clone());
    let file_data = std::fs::read(path.clone()).expect("failed to read the elf file");
    let slice = file_data.as_slice();
    let file = ElfBytes::<AnyEndian>::minimal_parse(slice).expect("failed to parse the elf file");

    let mut data: Vec<u8> = Vec::new();

    data.extend(LOAD_PROG_MAGIC);

    // let header = file.ehdr;
    // println!("type: {}", header.e_type);
    // println!("machine: {}", header.e_machine);
    // println!("entry: 0x{:X}", header.e_entry);

    let metadata = std::fs::metadata(path.clone()).unwrap();
    let size = metadata.size().to_le_bytes();
    println!("[PICOSH] program size {} {:?}", metadata.size(), size);
    data.extend(&size[0..8]);

    let symbol_table = file.symbol_table().unwrap().unwrap();
    let string_talbe = file.symbol_table().unwrap().unwrap().1;

    let symbols = symbol_table
        .0
        .into_iter()
        .find(|s| symbol.clone() == string_talbe.get(s.st_name as usize).unwrap())
        .unwrap_or_else(|| panic!("symbol with name {} not found", symbol.clone()));

    let symbol_address = symbols.st_value;
    let symbol_address_bytes = &symbol_address.to_le_bytes()[0..8];
    println!("[PICOSH] symbol address 0X{symbol_address:X} => {symbol_address_bytes:?}");

    data.extend_from_slice(symbol_address_bytes);

    let inter_trim: String = format!("{identifier:8}").chars().take(8).collect();
    let inter_bytes = inter_trim.clone().into_bytes();
    data.extend(&inter_bytes);
    println!(
        "[PICOSH] {} => {:?}, {}",
        inter_trim,
        inter_bytes,
        inter_bytes.len()
    );

    data.extend(file_data.as_slice());
    println!("[PICOSH] writing {} bytes do data", metadata.size());
    // data.extend(file_data.as_slice());

    // buf_writer.flush().expect("failed to flush file");

    Some(data)
}

fn handle_kill_cmd(identifier: String) -> Option<Vec<u8>> {
    let mut data: Vec<u8> = Vec::new();

    let ident_bytes = format!("{identifier:8}")
        .chars()
        .take(8)
        .collect::<String>()
        .into_bytes();

    println!(
        "[PICOSH] Attempting to kill program {:?} => {}",
        ident_bytes.clone(),
        String::from_utf8(ident_bytes.clone()).unwrap()
    );

    data.extend(KILL_PROG_MAGIC);
    data.extend(ident_bytes);

    Some(data)
}

fn handle_relaunch_cmd(identifier: String) -> Option<Vec<u8>> {
    let mut data: Vec<u8> = Vec::new();

    let ident_bytes = format!("{identifier:8}")
        .chars()
        .take(8)
        .collect::<String>()
        .into_bytes();

    println!(
        "[PICOSH] Attempting to kill program {:?} => {}",
        ident_bytes.clone(),
        String::from_utf8(ident_bytes.clone()).unwrap()
    );

    data.extend(RELOAD_TASKS_MAGIC);
    data.extend(ident_bytes);

    Some(data)
}

fn handle_list_cmd() -> Option<Vec<u8>> {
    let mut data: Vec<u8> = Vec::new();

    data.extend(LIST_TASKS_MAGIC);

    Some(data)
}

// #[allow(unused_assignments)]
fn handle_command(
    cmd: Commands,
    serial: Arc<Mutex<Box<dyn SerialPort + 'static>>>,
) -> Option<Vec<u8>> {
    let dump_path = std::path::PathBuf::from("/tmp/elf.dump");
    let mut dump_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dump_path)
        .expect("failed to create a dump file");

    let mut result: Vec<u8> = Vec::new();

    match cmd {
        Commands::Load {
            file,
            symbol,
            identifier,
        } => result = handle_load_cmd(file, symbol, identifier)?,
        Commands::List => result = handle_list_cmd()?,

        Commands::Relaunch { identifier } => result = handle_relaunch_cmd(identifier)?,
        Commands::Kill { identifier } => result = handle_kill_cmd(identifier)?,
        Commands::Log => {
            drop(result);
            return None;
        }
    }

    println!(
        "[PICOSH] writing {} bytes to dump file /tmp/elf.dump and serial device",
        result.len()
    );

    dump_file
        .write_all(&result)
        .expect("failed to write data to the dumpfile");

    let mut writer_handle = serial.lock().unwrap();
    for byte in result {
        writer_handle.write_all(&[byte]).unwrap();
        writer_handle.flush().unwrap();
    }

    writer_handle.flush().unwrap();
    None
}

fn main() {
    let args = Args::parse();

    let serial = Arc::new(Mutex::new(
        serialport::new(args.device.clone(), args.baudrate)
            .open()
            .unwrap_or_else(|_| panic!("unable to open device {}", args.device.clone())),
    ));

    let reader_serial = Arc::clone(&serial);
    // let write_serial = Arc::clone(&serial);

    thread::spawn(move || {
        loop {
            let handler_result = reader_serial.lock();

            if let Ok(mut handler) = handler_result {
                let mut read_buf: Vec<u8> = vec![0; 1];
                _ = handler.read_exact(read_buf.as_mut_slice());
                print!("{}", String::from_utf8_lossy(&read_buf));
                stdout().flush().unwrap();
            }
        }
    });

    sleep(time::Duration::from_secs(2));
    _ = handle_command(args.cmd, Arc::clone(&serial));

    loop {
        thread::sleep(time::Duration::from_secs(1));
    }
}
