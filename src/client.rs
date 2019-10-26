use lib::*;
//socket == "channel"
use clap::{Arg, App};

fn main() -> io::Result<()> {
    let mut stream = TcpStream::connect(("localhost", PORT_CLIENT_DAEMON)).unwrap();
    //Parsing arguments
    //share "file_path"
    //download "save_path" -fFileName (flag and save path in any order)
    //scan //ls //status
    let matches = App::new("ClientP2P")
                          .about("Interaction with daemon")
                          .arg(Arg::with_name("COMMAND")
                          .required(true)
                      )
                          .arg(Arg::with_name("FLG_BLOCK_INPUT")
                          .short("w")
                      )
                          .arg(Arg::with_name("FILE_NAME")    //Filename (download) - is option now
                          .short("f")
                          .takes_value(true)
                      )
                        /*  .arg(Arg::with_name("SAVE_PATH")    //Commented until I understood why
                          .short("o")
                          .takes_value(true)
                      )*/
                          .arg(Arg::with_name("FILE_PATH")
                      )
                          .get_matches();

    match matches.value_of("COMMAND").unwrap(){
        "share" => {
            if !matches.is_present("FILE_PATH"){
                panic!("No path for sharing!")
            }
            //println!("\n\n\tshare\n");
            //////////
            let f_path = String::from(matches.value_of("FILE_PATH").unwrap());
            let com = Command::Share{file_path: f_path};
            //
            let serialized = serde_json::to_string(&com)?;
            stream.write(serialized.as_bytes()).unwrap();
        },
        "scan" => {
            //println!("\n\n\tscan\n");
            //////////
            let com = Command::Scan;
            //
            let serialized = serde_json::to_string(&com)?;
            stream.write(serialized.as_bytes()).unwrap();
        },
        "ls" => {
            //println!("\n\n\tls\n");
            //////////
            let com = Command::Ls;
            //
            let serialized = serde_json::to_string(&com)?;
            stream.write(serialized.as_bytes()).unwrap();
        },
        "download" => {
            if !matches.is_present("FILE_NAME"){
                panic!("No file name to download!")
            }
            //
            let s_path: String;
            if matches.is_present("FILE_PATH"){
                s_path = String::from(matches.value_of("FILE_PATH").unwrap());
            }
            else{
                s_path = String::from("");
            }
            //
            let f_name: String = String::from(matches.value_of("FILE_NAME").unwrap());
            //println!("\n\n\tls\n");
            //////////
            let com = Command::Download{file_name: f_name, save_path: s_path};
            //
            let serialized = serde_json::to_string(&com)?;
            stream.write(serialized.as_bytes()).unwrap();
        },
        "status" => {
            //println!("\n\n\tstatus\n");
            //////////
            let com = Command::Status;
            //
            let serialized = serde_json::to_string(&com)?;
            stream.write(serialized.as_bytes()).unwrap();
        },
        _ => {
            panic!("Wrong command!");
        }

    }

    let mut buf = vec![0 as u8; 4096];
    match stream.read(&mut buf) {
        Ok(size) => {
            let answ: Answer = serde_json::from_slice(&buf[..size])?;
            println!("{:?}", answ);
        },
        Err(_) => {
            println!("An error occurred, {}", stream.peer_addr().unwrap());
        }
    }

    Ok(())
}
