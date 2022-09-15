use anyhow::*;
use futures::StreamExt;
use quinn::{
    Certificate, CertificateChain, ClientConfigBuilder, Connecting, Endpoint, NewConnection,
    PrivateKey, ServerConfig, ServerConfigBuilder, TransportConfig,
};
use rand::AsByteSliceMut;
//use rand::{thread_rng, Rng};
use std::fs::File;
use std::path::Path;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
use std::result::Result::Ok;

// 送信用のデータ(100KiB)
const MSG_SIZE: usize = 1024 * 100000000;

#[tokio::main]
async fn main() -> Result<()> {
    let cert = rcgen::generate_simple_self_signed(["localhost".to_owned()])?;
    let cert_der = cert.serialize_der()?;
    let priv_key = cert.serialize_private_key_der();

    let cert_der_clone = cert_der.clone();

    // 送信用の適当なデータを読み込む
    let filename = "Cargo.toml.zip";
    let mut f = File::open(filename).expect("file not found");
    let mut send_data = vec![];
    f.read_to_end(&mut send_data)
        .expect("something went wrong reading the file");

    // サーバを動かす

    tokio::spawn(async move {
        run_server(cert_der_clone, priv_key).await.unwrap();
    });
    
    // クライアントを動かし、所要時間(ミリ秒)を表示する
    // let start = Instant::now();
    // run_client(cert_der, &send_data).await?;
    // let elapsed = start.elapsed();
    // println!(
    //     "Elapsed time: {} ms",
    //     elapsed.as_secs() * 1000 + elapsed.subsec_millis() as u64
    // );

    Ok(())
}

// サーバを動かす
async fn run_server(cert_der: Vec<u8>, priv_key: Vec<u8>) -> Result<()> {
    let mut transport_config = TransportConfig::default();
    transport_config.stream_window_uni(0xFF);
    let mut server_config = ServerConfig::default();
    server_config.transport = std::sync::Arc::new(transport_config);
    let mut server_config = ServerConfigBuilder::new(server_config);
    let cert = Certificate::from_der(&cert_der)?;
    server_config.certificate(
        CertificateChain::from_certs(vec![cert]),
        PrivateKey::from_der(&priv_key)?,
    )?;
    let mut endpoint = Endpoint::builder();
    endpoint.listen(server_config.build());
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 33333);
    let (endpoint, mut incoming) = endpoint.bind(&addr)?;
    println!("listeing on {}", endpoint.local_addr()?);

    while let Some(conn) = incoming.next().await {
        tokio::spawn(async {
            match handle_connection(conn).await {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("{}", e);
                }
            }
        });
    }

    Ok(())
}

// サーバへの接続を扱う
async fn handle_connection(conn: Connecting) -> Result<(), Error> {
    let NewConnection {
        connection,
        mut uni_streams,
        ..
    } = conn.await?;

    println!("connected from {}", connection.remote_address());

    while let Some(uni_stream) = uni_streams.next().await {
        let uni_stream = uni_stream?;
        tokio::spawn(async {
            let _data = uni_stream.read_to_end(MSG_SIZE).await.unwrap();
            //let converted: String = String::from_utf8(_data.to_vec()).unwrap();
            println!("{:?}", _data);
            let path = Path::new("send_test.zip");
            let display = path.display();
        
            let mut file = match File::create(&path) {
                Err(why) => panic!("couldn't create {}: {}", display, why),
                Ok(file) => file,
            };
        
            match file.write_all(&_data) {
                Err(why) => panic!("couldn't write to {}: {}", display, why),
                Ok(_) => println!("successfully wrote to {}", display),
            }
        });
    }

    println!("connection closed from {}", connection.remote_address());

    Ok(())
}

// クライアントを動かす
async fn run_client(cert_der: Vec<u8>, send_data: &[u8]) -> Result<()> {
    let mut client_config = ClientConfigBuilder::default();
    client_config.add_certificate_authority(Certificate::from_der(&cert_der)?)?;
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 33333);
    let mut endpoint_builder = Endpoint::builder();
    endpoint_builder.default_client_config(client_config.build());
    let (endpoint, _incoming) = endpoint_builder.bind(&"0.0.0.0:0".parse().unwrap())?;

    let NewConnection { connection, .. } = endpoint.connect(&addr, "localhost")?.await?;
    println!("connected: addr={}", connection.remote_address());

    let mut tasks = futures::stream::FuturesUnordered::new();

    {
        let connection = connection.clone();
        let send_data = send_data.to_vec();
        let task = async move {
            let mut send_stream = connection.open_uni().await.unwrap();
            send_stream.write_all(&send_data).await.unwrap();
            send_stream.finish().await.unwrap();
        };
        tasks.push(task);
    }

    while let Some(_) = tasks.next().await {}

    connection.close(0u8.into(), &[]);

    endpoint.wait_idle().await;

    Ok(())
}