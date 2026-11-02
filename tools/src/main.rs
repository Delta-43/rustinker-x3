use std::{error::Error, fs, io::Read};
use x3_tools::{Fit, Orientation};
fn read(path: &str, cap: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut b = Vec::new();
    fs::File::open(path)?.take(cap + 1).read_to_end(&mut b)?;
    if b.len() as u64 > cap {
        return Err("input too large".into());
    }
    Ok(b)
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str){
 Some("token") if args.len()==5=>{let fit=match args[3].as_str(){"contain"=>Fit::Contain,"cover"=>Fit::Cover,_=>return Err("fit must be contain or cover".into())};let orientation=match args[4].as_str(){"auto"=>Orientation::Auto,"landscape"=>Orientation::Landscape,"portrait"=>Orientation::Portrait,_=>return Err("invalid orientation".into())};let b=x3_tools::token(&read(&args[1],8*1024*1024)?,fit,orientation)?;fs::write(&args[2],b)?;}
 Some("article") if args.len()==6=>{let b=read(&args[1],1024*1024)?;let summary=match args[5].as_str(){"article"=>false,"summary"=>true,_=>return Err("kind must be article or summary".into())};let time=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();let b=x3_tools::article(&args[3],&args[4],std::str::from_utf8(&b)?,summary,time)?;fs::write(&args[2],b)?;}
 Some("flash-info") if args.len()==2=>{print!("{}",x3_tools::flash::report(&read(&args[1],16*1024*1024)?));}
 Some("otadata-select") if args.len()==4=>{let slot=args[2].parse().map_err(|_|"slot must be a number")?;fs::write(&args[3],x3_tools::flash::otadata_select(&read(&args[1],16*1024*1024)?,slot)?)?;}
 Some("font") if args.len()==4=>{let px:f32=args[2].parse().map_err(|_|"size must be a number")?;let b=x3_tools::font::rasterise(&read(&args[1],4*1024*1024)?,px)?;fs::write(&args[3],&b)?;println!("{} bytes",b.len());}
 Some("put-article") if args.len()==4=>{let r=x3_tools::store::put_article(std::path::Path::new(&args[1]),&args[2],&read(&args[3],x3_proto::ARTICLE_CAP as u64)?)?;println!("stored {} as generation {}, {} bytes",r.item(),r.generation,r.len);}
 Some("preview") if args.len()==3=>{x3_tools::token_preview(&read(&args[1],x3_proto::TOKEN_CAP as u64)?)?.save(&args[2])?;}
 _=>return Err("usage: x3-tools token INPUT OUTPUT.tk1 contain|cover auto|landscape|portrait; article INPUT.txt OUTPUT.ar1 TITLE URL article|summary; preview INPUT.tk1 OUTPUT.png; flash-info DUMP.bin; otadata-select DUMP.bin SLOT OUT.bin; font TTF PX OUT.x3f; put-article DIR ID FILE.ar1".into())
 }
    Ok(())
}
