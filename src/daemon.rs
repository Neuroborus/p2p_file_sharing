use lib::*;

///
fn command_processor(
    com: &Command,
    mut stream: TcpStream,
    mut data: MutexGuard<DataTemp>,
    transferring: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>,
    downloading: Arc<Mutex<Vec<String>>>,
) -> io::Result<()> {
    match com {
        Command::Share { file_path: f_path } => {
            println!("!Share!");

            let name: String = String::from(f_path.file_name().unwrap().to_string_lossy());
            data.shared.insert(name, f_path.clone()); //Name - path

            let answ = Answer::Ok;
            let serialized = serde_json::to_string(&answ)?;
            stream.write_all(serialized.as_bytes()).unwrap();
        }
        Command::Download {
            file_name: f_name,
            save_path: s_path,
            wait: wat,
        } => {
            println!("!Download!");

            let answ: Answer;

            if data.available.contains_key(f_name) == false {
                answ = Answer::Err(String::from("File is not available to download!"));
            } else if data.shared.contains_key(f_name) == true {
                answ = Answer::Err(String::from("You already own this file, and even sharing!"));
            } else {
                answ = Answer::Ok;
            }

            match answ {
                Answer::Ok => {
                    let available_list = data.available.get(f_name).unwrap().clone();
                    let filename = f_name.clone();
                    let savepath = s_path.clone();
                    let file_thread = thread::spawn(move || {
                        download_request(filename, savepath, available_list, downloading).unwrap();
                    });
                    if *wat == true {
                        file_thread.join().unwrap();
                    }
                }
                _ => {}
            }

            let serialized = serde_json::to_string(&answ)?;
            stream.write_all(serialized.as_bytes()).unwrap();
        }
        Command::Scan => {
            println!("!Scan!");
            let socket = UdpSocket::bind((Ipv4Addr::new(0, 0, 0, 0), 0))?;
            socket.send_to(SCAN_REQUEST, (ADDR_DAEMON_MULTICAST, PORT_MULTICAST))?;
            data.available.clear(); // clear list of available files to download
                                    //the list gonna be refreshed in multicast_receiver

            let answ = Answer::Ok;
            let serialized = serde_json::to_string(&answ)?;
            stream.write_all(serialized.as_bytes()).unwrap();
        }
        Command::Ls => {
            println!("!Ls!");
            let answ: Answer = Answer::Ls {
                available_map: data.available.clone(),
            };
            let serialized = serde_json::to_string(&answ)?;
            stream.write_all(serialized.as_bytes()).unwrap();
        }
        Command::Status => {
            println!("!Status!"); //transferring & shared
            let answ: Answer = Answer::Status {
                transferring_map: transferring.lock().unwrap().clone(),
                shared_map: data.shared.clone(),
                downloading_map: downloading.lock().unwrap().clone(),
            };
            let serialized = serde_json::to_string(&answ)?;
            stream.write_all(serialized.as_bytes()).unwrap();
        }
    }

    println!("{:?}", data);

    Ok(())
}

fn multicast_responder(data: Arc<Mutex<DataTemp>>) -> io::Result<()> {
    let this_daemon_ip = get_this_daemon_ip().unwrap();

    let listener = bind_multicast(&ADDR_DAEMON_MULTICAST, PORT_MULTICAST)?;
    listener.join_multicast_v4(&ADDR_DAEMON_MULTICAST, &Ipv4Addr::new(0, 0, 0, 0))?;

    let mut shared: Vec<String>;
    let mut buf = vec![0; 4096];
    loop {
        //println!("MLTCST_RESPOND PART 1");
        let (len, remote_addr) = listener.recv_from(&mut buf)?;
        //println!("MLTCST_RESPOND PART 2 {}", remote_addr);
        if remote_addr.ip() != this_daemon_ip {
            //check if that's not our daemon, then we will respond
            let message = &buf[..len];
            let mut stream = TcpStream::connect((remote_addr.ip(), PORT_SCAN_TCP))?;
            println!("MULTICAST RESPONDING TO {}", remote_addr.ip());

            if message == SCAN_REQUEST {
                let dat = data.lock().unwrap();
                shared = Vec::new();
                for key in dat.shared.keys() {
                    shared.push(key.clone());
                }
                let serialized = serde_json::to_string(&shared)?;
                stream.write_all(serialized.as_bytes()).unwrap(); //Send our "shared"
            }
        }
    }
}

fn multicast_receiver(data: Arc<Mutex<DataTemp>>) -> io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::new(0, 0, 0, 0), PORT_SCAN_TCP))?;
    //get names of files with tcp (his shared - your available)
    let mut buf = vec![0 as u8; 4096];
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                match stream.read(&mut buf) {
                    Ok(size) => {
                        //Get List of names
                        let names: Vec<String> = serde_json::from_slice(&buf[..size])?;
                        for name in names.into_iter() {
                            if data.lock().unwrap().available.contains_key(&name) {
                                //If file already exist just update Vec of IP
                                data.lock()
                                    .unwrap()
                                    .available
                                    .get_mut(&name)
                                    .unwrap()
                                    .push(stream.peer_addr().unwrap());
                            } else {
                                //In another case - adding file with first IP that share it
                                let mut v: Vec<SocketAddr> = Vec::new();
                                v.push(stream.peer_addr().unwrap());
                                data.lock().unwrap().available.insert(name, v);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("An error occurred, {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
    Ok(())
}

fn share_responder(
    transferring: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>,
    data: Arc<Mutex<DataTemp>>,
) -> io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::new(0, 0, 0, 0), PORT_FILE_SHARE))?;

    for stream in listener.incoming() {
        match stream {
            Ok(mut _stream) => {
                let peer_transferring = transferring.clone();
                let shared = data.lock().unwrap().shared.clone();
                println!("{:?} asked to share a file", _stream.peer_addr());
                thread::spawn(move || {
                    share_to_peer(_stream, peer_transferring, shared).unwrap();
                });
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
    Ok(())
}

fn share_to_peer(
    mut stream: TcpStream,
    transferring: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>,
    shared: HashMap<String, PathBuf>,
) -> io::Result<()> {
    let mut buf = vec![0; 4096];

    stream.set_read_timeout(Some(Duration::new(45, 0)))?;
    stream.set_write_timeout(Some(Duration::new(45, 0)))?;

    let file_name: String;
    let file_info: FileInfo;
    let file_size: u64;

    match stream.read(&mut buf) {
        Ok(size) => {
            let request: FirstRequest = serde_json::from_slice(&buf[..size])?;
            let asked_filename: String = request.filename;
            match request.action {
                FileSizeorInfo::Size => {
                    let answ: AnswerToFirstRequest;
                    if shared.contains_key(&asked_filename) == false {
                        answ = AnswerToFirstRequest {
                            filename: asked_filename.clone(),
                            answer: EnumAnswer::NotExist,
                        };
                    } else {
                        let size_of_file: u64 =
                            std::fs::metadata(shared.get(&asked_filename).unwrap())?.len(); //get file size
                        answ = AnswerToFirstRequest {
                            filename: asked_filename.clone(),
                            answer: EnumAnswer::Size(size_of_file),
                        };
                    }
                    let serialized = serde_json::to_string(&answ)?;
                    stream.write_all(serialized.as_bytes()).unwrap();
                    return Ok(());
                }
                FileSizeorInfo::Info(info) => {
                    file_info = info;
                    file_name = asked_filename.clone();
                    file_size = std::fs::metadata(shared.get(&asked_filename).unwrap())?.len();
                }
            }
        }
        Err(e) => {
            eprintln!("An error occurred, {}", e);
            return Err(e);
        }
    }

    let blocks: u32;

    {
        let mut transfer_map = transferring.lock().unwrap();
        match transfer_map.get_mut(&file_name) {
            Some(addr_vec) => {
                addr_vec.push(stream.peer_addr().unwrap().clone());
            }
            None => {
                let mut v: Vec<SocketAddr> = Vec::new();
                v.push(stream.peer_addr().unwrap());
                transfer_map.insert(file_name.clone(), v);
            }
        }
    }

    blocks = (file_size / 4096) as u32;
    let last_block_size = (file_size % 4096) as usize;

    let mut file = fs::File::open(&file_name)?;
    file.seek(SeekFrom::Start(4096 * file_info.from_block as u64))?;
    for i in file_info.from_block..file_info.to_block {
        if i == blocks {
            if last_block_size == 0 {
                break;
            }
            buf.resize(last_block_size, 0u8);
        }
        file.read_exact(&mut buf)?;
        stream.write_all(&buf).unwrap();
    }
    {
        let mut transfer_map = transferring.lock().unwrap();
        if transfer_map.get(&file_name).unwrap().len() == 1 {
            transfer_map.remove(&file_name).unwrap();
        } else {
            let peer_vec: &mut Vec<SocketAddr> = transfer_map.get_mut(&file_name).unwrap();
            let pos: usize = peer_vec
                .iter()
                .position(|&peer| peer == stream.peer_addr().unwrap())
                .unwrap();
            peer_vec.remove(pos);
        }
    }

    Ok(())
}

fn get_fsize_on_each_peer(peer_list: Vec<SocketAddr>, file_name: String) -> io::Result<Vec<(SocketAddr, u64)>> {
    let mut buf = vec![0; 4096];
    let mut peers: Vec<(SocketAddr, u64)> = Vec::new();
    let request_to_get_size = serde_json::to_string(&FirstRequest {
        filename: file_name.clone(),
        action: FileSizeorInfo::Size,
    })?;
    let mut refresh = true;

    for peer in peer_list.iter() {
        let mut stream: TcpStream;
        match TcpStream::connect((peer.ip(), PORT_FILE_SHARE)) {
            Ok(_stream) => {
                stream = _stream;
            }
            Err(e) => {
                eprintln!("Error while connecting to {} to download a file {}", peer.ip(), e);
                continue;
            }
        }
        stream.set_read_timeout(Some(Duration::new(30, 0)))?;
        stream.set_write_timeout(Some(Duration::new(30, 0)))?;
        stream.write_all(request_to_get_size.as_bytes())?;
        match stream.read(&mut buf) {
            Ok(size) => {
                let answer: AnswerToFirstRequest = serde_json::from_slice(&buf[..size])?;
                match answer.answer {
                    EnumAnswer::Size(file_size) => {
                        peers.push((stream.peer_addr()?, file_size));
                    }
                    EnumAnswer::NotExist => {
                        if refresh {
                            println!("That peer doesn't share a file! Please refresh list of files with scan!");
                            refresh = false;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Error {} while interracting with {:?}",
                    e,
                    stream.peer_addr()
                );
            }
        }
    }
    Ok(peers)
}

fn download_request(
    file_name: String,
    file_path: PathBuf,
    available: Vec<SocketAddr>,
    downloading: Arc<Mutex<Vec<String>>>,
) -> io::Result<()> {
    
    let mut peers: Vec<(SocketAddr, u64)> = get_fsize_on_each_peer(available, file_name.clone()).unwrap();

    let file_size: u64;
    let peers_count: u16;
    let prs = peers.clone();
    {
        let mut _max_peers: u16 = 1;
        let mut fsize_count: Vec<(u64, u16)> = Vec::new();
        prs.iter().for_each( |(_, fsize)| {
            let buffer_var = fsize_count.iter_mut().find(|(size, _)| *size == *fsize);
            if buffer_var == Option::None {
                fsize_count.push((*fsize, 1));
            } else {
                buffer_var.unwrap().1 += 1;
            }
        });
        _max_peers = fsize_count.iter().max_by_key(|(_, count)| *count).unwrap().1;
        file_size = fsize_count.iter().find(|(_, count)| _max_peers == *count).unwrap().0;
        peers_count = _max_peers;
    }
    peers.retain(|(_peer, size)| *size == file_size); // removes peers with other file size

    let blocks = (file_size / 4096) as u32;
    let file_size: u64 = file_size;
    let blocks_per_peer = blocks / (peers_count as u32);

    downloading.lock().unwrap().push(file_name.clone());

    let pool: ThreadPool;

    let downloaded_blocks: Arc<Mutex<Vec<(u32, u32)>>> = Arc::new(Mutex::new(Vec::new()));

    if  (blocks as usize) < peers.len() {
        let file_info = FirstRequest {
            filename: file_name.clone(),
            action: FileSizeorInfo::Info(FileInfo {
                from_block: 0,
                to_block: blocks + 1,
            }),
        };
        let fpath = file_path.clone();
        let block_watcher = downloaded_blocks.clone();
        let _fsize = file_size;
        let _blocks = blocks;

        pool = ThreadPool::new(1);
        pool.execute(move || {
            download_from_peer(
                peers[0].0.clone(),
                file_info,
                fpath,
                _fsize,
                _blocks,
                block_watcher,
            )
            .unwrap();
        });
    } else {
        pool = ThreadPool::new(peers.len());

        for i in 0..(peers.len() as u32) {
            let fblock = i * blocks_per_peer;
            let mut lblock = (i + 1) * blocks_per_peer;
            if i == (peers_count as u32) - 1 && lblock != blocks + 1 {
                lblock = blocks + 1;
            }
            let file_info = FirstRequest {
                filename: file_name.clone(),
                action: FileSizeorInfo::Info(FileInfo {
                    from_block: fblock,
                    to_block: lblock,
                }),
            };

            let fpath = file_path.clone();
            let block_watcher = downloaded_blocks.clone();
            let _fsize = file_size;
            let _blocks = blocks;
            let pr = peers[i as usize].0.clone();
            pool.execute(move || {
                download_from_peer(
                    pr,
                    file_info,
                    fpath,
                    _fsize,
                    _blocks,
                    block_watcher,
                )
                .unwrap();
            });
        }
    }
    pool.join();
    downloading.lock().unwrap().retain(|line| *line != file_name);
    Ok(())
}

fn download_from_peer(
    peer: SocketAddr,
    file_info: FirstRequest,
    _file_path: PathBuf,
    file_size: u64,
    file_blocks: u32,
    _block_watcher: Arc<Mutex<Vec<(u32, u32)>>>,
) -> io::Result<()> {
    let mut stream = TcpStream::connect((peer.ip(), PORT_FILE_SHARE)).unwrap();

    let mut buf = vec![0u8; 4096];

    stream.set_read_timeout(Some(Duration::new(45, 0)))?;
    stream.set_write_timeout(Some(Duration::new(45, 0)))?;

    let ser = serde_json::to_string(&file_info).unwrap();
    stream.write_all(ser.as_bytes()).unwrap();

    let fblock: u32;
    let lblock: u32;

    match file_info.action {
        FileSizeorInfo::Info(obj) => {
            fblock = obj.from_block;
            lblock = obj.to_block;
        }
        _ => {
            fblock = 0;
            lblock = 0;
        }
    }

    let last_block_size = file_size as usize % 4096;
    let mut file = fs::File::create(&file_info.filename)?;
    file.seek(SeekFrom::Start(4096 * fblock as u64))?;
    for i in fblock..lblock {
        if i == file_blocks {
            if last_block_size == 0 {
                break;
            }
            buf.resize(last_block_size, 0u8);
        }
        stream.read_exact(&mut buf).unwrap();
        file.write_all(&buf)?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    println!("Daemon: running");
    //Listener for client-daemon connection
    let listener = TcpListener::bind(("localhost", PORT_CLIENT_DAEMON))?;
    //All about files daemon knowledge
    let data: Arc<Mutex<DataTemp>> = Arc::new(Mutex::new(DataTemp::new()));
    //Channel for transfeering info about sharing to multicast_responder
    let transferring: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let downloading: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let mult_resp_data = data.clone();
    thread::spawn(move || {
        multicast_responder(mult_resp_data).unwrap();
    });

    let mult_recv_data = data.clone();
    thread::spawn(move || {
        multicast_receiver(mult_recv_data).unwrap();
    });

    let share_transfer = transferring.clone();
    let share_data = data.clone();
    thread::spawn(move || {
        share_responder(share_transfer, share_data).unwrap();
    });

    //
    let mut buf = vec![0 as u8; 4096];
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                /////////////
                match stream.read(&mut buf) {
                    Ok(size) => {
                        //Now the daemon does not crash when the command is entered incorrectly
                        let com: Command;
                        match serde_json::from_slice(&buf[..size]) {
                            Ok(c) => {
                                com = c;
                            }
                            Err(_) => {
                                println!("Client made a mistake!");
                                continue;
                            }
                        }

                        println!("{:?}", *data);
                        let dat = data.clone();
                        let com_transfer = transferring.clone();
                        let com_download = downloading.clone();
                        thread::spawn(move || {
                            command_processor(
                                &com,
                                stream,
                                dat.lock().unwrap(),
                                com_transfer,
                                com_download,
                            )
                            .unwrap();
                        });
                    }
                    Err(e) => {
                        eprintln!("An error occurred, {}", e);
                    }
                }
                ///////////////
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_share() {
        let _listener = TcpListener::bind(("localhost", PORT_CLIENT_DAEMON + 1)).unwrap();
        //
        let mut stream = TcpStream::connect(("localhost", PORT_CLIENT_DAEMON + 1)).unwrap();
        let com = Command::Share {
            file_path: PathBuf::from("fake\\path\\FILE.dat"),
        };
        let stream_tmp = _listener.accept().unwrap().0;
        let dat: Arc<Mutex<DataTemp>> = Arc::new(Mutex::new(DataTemp::new()));

        let tr: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> = Arc::new(Mutex::new(HashMap::new()));
        let dow: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        command_processor(&com, stream_tmp, dat.lock().unwrap(), tr, dow).unwrap();

        let mut buf = vec![0 as u8; 4096];
        match stream.read(&mut buf) {
            Ok(size) => {
                let answ: Answer = serde_json::from_slice(&buf[..size]).unwrap();
                assert_eq!(answ, Answer::Ok);
                //Checking dat after changes
                assert_eq!(
                    (dat.lock().unwrap())
                        .shared
                        .get(&String::from("FILE.dat"))
                        .unwrap(),
                    &PathBuf::from("fake\\path\\FILE.dat")
                );
            }
            Err(_) => {
                panic!("An error occurred, {}", stream.peer_addr().unwrap());
            }
        }
    }

    #[test]
    fn test_download() {
        let _listener = TcpListener::bind(("localhost", PORT_CLIENT_DAEMON + 2)).unwrap();
        //
        let mut stream = TcpStream::connect(("localhost", PORT_CLIENT_DAEMON + 2)).unwrap();
        let com = Command::Download {
            file_name: String::from("FILE.dat"),
            save_path: PathBuf::from("fake\\path\\"),
            wait: false,
        };
        let stream_tmp = _listener.accept().unwrap().0;
        let dat: Arc<Mutex<DataTemp>> = Arc::new(Mutex::new(DataTemp::new()));

        let tr: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> = Arc::new(Mutex::new(HashMap::new()));
        let dow: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        command_processor(&com, stream_tmp, dat.lock().unwrap(), tr, dow).unwrap();

        let mut buf = vec![0 as u8; 4096];
        match stream.read(&mut buf) {
            Ok(size) => {
                let answ: Answer = serde_json::from_slice(&buf[..size]).unwrap();
                assert_eq!(
                    answ,
                    Answer::Err(String::from("File is not available to download!"))
                );
            }
            Err(e) => panic!("An error occurred, {}", e),
        }
    }

    #[test]
    fn test_scan() {
        let _listener = TcpListener::bind(("localhost", PORT_CLIENT_DAEMON + 3)).unwrap();
        //
        let mut stream = TcpStream::connect(("localhost", PORT_CLIENT_DAEMON + 3)).unwrap();
        let com = Command::Scan;
        let stream_tmp = _listener.accept().unwrap().0;
        let dat: Arc<Mutex<DataTemp>> = Arc::new(Mutex::new(DataTemp::new()));

        let tr: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> = Arc::new(Mutex::new(HashMap::new()));
        let dow: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        command_processor(&com, stream_tmp, dat.lock().unwrap(), tr, dow).unwrap();

        let mut buf = vec![0 as u8; 4096];
        match stream.read(&mut buf) {
            Ok(size) => {
                let answ: Answer = serde_json::from_slice(&buf[..size]).unwrap();
                assert_eq!(answ, Answer::Ok);
            }
            Err(_) => {
                panic!("An error occurred, {}", stream.peer_addr().unwrap());
            }
        }
    }

    #[test]
    fn test_ls() {
        let _listener = TcpListener::bind(("localhost", PORT_CLIENT_DAEMON + 4)).unwrap();
        //
        let mut stream = TcpStream::connect(("localhost", PORT_CLIENT_DAEMON + 4)).unwrap();
        let com = Command::Ls;
        let stream_tmp = _listener.accept().unwrap().0;
        let dat: Arc<Mutex<DataTemp>> = Arc::new(Mutex::new(DataTemp::new()));

        dat.lock()
            .unwrap()
            .available
            .insert(String::from("FILE.dat"), Vec::new());

        let tr: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> = Arc::new(Mutex::new(HashMap::new()));
        let dow: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        command_processor(&com, stream_tmp, dat.lock().unwrap(), tr, dow).unwrap();

        let mut buf = vec![0 as u8; 4096];
        match stream.read(&mut buf) {
            Ok(size) => {
                let answ: Answer = serde_json::from_slice(&buf[..size]).unwrap();
                //
                let mut t: HashMap<String, Vec<SocketAddr>> = HashMap::new();
                t.insert(String::from("FILE.dat"), Vec::new());

                let tmp: Answer = Answer::Ls { available_map: t };
                assert_eq!(answ, tmp);
            }
            Err(_) => {
                panic!("An error occurred, {}", stream.peer_addr().unwrap());
            }
        }
    }

    #[test]
    fn test_status() {
        let _listener = TcpListener::bind(("localhost", PORT_CLIENT_DAEMON + 5)).unwrap();
        //
        let mut stream = TcpStream::connect(("localhost", PORT_CLIENT_DAEMON + 5)).unwrap();
        let com = Command::Status;
        let stream_tmp = _listener.accept().unwrap().0;
        let dat: Arc<Mutex<DataTemp>> = Arc::new(Mutex::new(DataTemp::new()));

        dat.lock().unwrap().shared.insert(
            String::from("FILE1.dat"),
            PathBuf::from("fake\\path\\FILE1.dat"),
        );

        let tr: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> = Arc::new(Mutex::new(HashMap::new()));
        tr.lock()
            .unwrap()
            .insert(String::from("FILE.dat"), Vec::new());

        let dow: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        command_processor(&com, stream_tmp, dat.lock().unwrap(), tr, dow).unwrap();

        let mut buf = vec![0 as u8; 4096];
        match stream.read(&mut buf) {
            Ok(size) => {
                let answ: Answer = serde_json::from_slice(&buf[..size]).unwrap();
                //
                let mut tr: HashMap<String, Vec<SocketAddr>> = HashMap::new();
                tr.insert(String::from("FILE.dat"), Vec::new());

                let mut sh: HashMap<String, PathBuf> = HashMap::new();
                sh.insert(
                    String::from("FILE1.dat"),
                    PathBuf::from("fake\\path\\FILE1.dat"),
                );

                let dow_t: Vec<String> = Vec::new();

                let tmp: Answer = Answer::Status {
                    transferring_map: tr,
                    shared_map: sh,
                    downloading_map: dow_t,
                };
                assert_eq!(answ, tmp);
            }
            Err(_) => {
                panic!("An error occurred, {}", stream.peer_addr().unwrap());
            }
        }
    }
}
