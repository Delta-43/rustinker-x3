use base64::Engine;
use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use x3_companion::{
    auth::Auth,
    db::Database,
    server::{self, App},
};
fn var(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
fn data_dir() -> PathBuf {
    PathBuf::from(var("X3_DATA_DIR", "data"))
}
async fn maintenance(db: Arc<Database>, directory: PathBuf) {
    let mut last_day = -1;
    loop {
        let day = x3_companion::now() / 86400;
        if day != last_day {
            let backup_dir = directory.join("backups");
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                std::fs::create_dir_all(&backup_dir)?;
                let path = backup_dir.join(format!("inbox-{day:010}.sqlite"));
                if !path.exists() {
                    db.backup(&path)?;
                }
                Database::restore_check(&path)?;
                let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("inbox-") && n.ends_with(".sqlite"))
                    })
                    .collect();
                backups.sort();
                let excess = backups.len().saturating_sub(7);
                for p in backups.into_iter().take(excess) {
                    std::fs::remove_file(p)?;
                }
                db.prune(30)?;
                Ok(())
            })();
            if result.is_ok() {
                last_day = day;
            } else {
                eprintln!("Daily backup or retention failed; will retry.");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "restore-check") && args.len() == 2 {
        let count = Database::restore_check(Path::new(&args[1]))?;
        println!("Backup verified: {count} payloads.");
        return Ok(());
    }
    let directory = data_dir();
    std::fs::create_dir_all(&directory)?;
    let quota = var("X3_QUOTA_BYTES", "536870912").parse()?;
    let db = Arc::new(Database::open(directory.join("inbox.sqlite"), quota)?);
    match args.first().map(String::as_str){
 Some("device") if args.len()==3 && args[1]=="add"=>{let mut random=[0;32];getrandom::fill(&mut random).map_err(|_|"Random source unavailable")?;let token=base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random);db.add_device(&args[2],&token)?;println!("{token}");}
 Some("backup") if args.len()==2=>{db.backup(Path::new(&args[1]))?;println!("Backup complete.");}
 Some("serve") if args.len()==1 || (args.len()==2&&args[1]=="--dev-insecure")=>{
 let public:SocketAddr=var("X3_PUBLIC_BIND","127.0.0.1:8787").parse()?;let device:SocketAddr=var("X3_DEVICE_BIND","127.0.0.1:8788").parse()?;
 let auth=if args.len()==2{if !device.ip().is_loopback(){return Err("Development listeners must use loopback".into());}Auth::dev(public,&env::var("X3_DEV_TOKEN").map_err(|_|"Set X3_DEV_TOKEN for local development")?)?}else{Auth::access(&env::var("X3_ACCESS_TEAM").map_err(|_|"Set X3_ACCESS_TEAM")?,&env::var("X3_ACCESS_AUD").map_err(|_|"Set X3_ACCESS_AUD")?)?};
 if db.device()?.is_null(){return Err("Add the device before starting the server".into());}
 let signed:Option<SocketAddr>=env::var("X3_SIGNED_BIND").ok().map(|v|v.parse()).transpose()?;
 let a=tokio::net::TcpListener::bind(public).await?;let b=tokio::net::TcpListener::bind(device).await?;let c=match signed{Some(s)=>Some(tokio::net::TcpListener::bind(s).await?),None=>None};
 db.restart()?;let mut app=App::new(db.clone(),auth)?;app.fonts=env::var_os("X3_FONT_DIR").map(std::path::PathBuf::from);let worker=tokio::spawn(server::worker(app.clone()));let backup=tokio::spawn(maintenance(db,directory));
 println!("Public listener: {public}; device listener: {device}");
 if let Some(s)=signed{println!("Signed device listener (for the proxy): {s}");}
 let signed_app=app.clone();let signed_server=async move{match c{Some(c)=>axum::serve(c,server::device_signed(signed_app)).await,None=>std::future::pending().await}};
 tokio::select!{r=signed_server=>{r?;},r=axum::serve(a,server::public(app.clone()))=>{r?;},r=axum::serve(b,server::device(app))=>{r?;},_=tokio::signal::ctrl_c()=>{}}
 worker.abort();backup.abort();
 }
 _=>return Err("usage: x3-companion serve [--dev-insecure] | device add NAME | backup PATH | restore-check PATH".into())
 }
    Ok(())
}
