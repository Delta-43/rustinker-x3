fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|a| a == "receive") {
        if arguments.len() != 4 {
            return Err("usage: sim receive SERVER.cfg DISK.img OUTPUT_DIR".into());
        }
        use std::io::Read;
        let mut config = Vec::new();
        std::fs::File::open(&arguments[1])?
            .take(513)
            .read_to_end(&mut config)?;
        let count = sim::receive::receive(
            &config,
            std::path::Path::new(&arguments[2]),
            std::path::Path::new(&arguments[3]),
        )?;
        println!(
            "Receive complete: {count} new payloads; PNGs in {}",
            arguments[3]
        );
        return Ok(());
    }
    if arguments.first().is_some_and(|a| a == "gui") {
        let dir = arguments.get(1).map_or("samples/gui", String::as_str);
        std::fs::create_dir_all(dir)?;
        for (name, image) in sim::gui_screens().map_err(|e| format!("{e:?}"))? {
            image.save(format!("{dir}/{name}.png"))?;
        }
        println!("GUI screens in {dir}");
        return Ok(());
    }
    let mut args = arguments.into_iter();
    let dir = args.next().unwrap_or_else(|| "samples".into());
    if args.next().is_some() {
        return Err("usage: sim [output-directory]".into());
    }
    std::fs::create_dir_all(&dir)?;
    sim::token(1)
        .map_err(|e| format!("{e:?}"))?
        .save(format!("{dir}/token-1.png"))?;
    sim::token(2)
        .map_err(|e| format!("{e:?}"))?
        .save(format!("{dir}/token-2.png"))?;
    let (page, _) = sim::article(sim::ARTICLE, 0).map_err(|e| format!("{e:?}"))?;
    page.save(format!("{dir}/article.png"))?;
    let long = [sim::ARTICLE; 3].join("\n\n");
    let (_, next) = sim::article(&long, 0).map_err(|e| format!("{e:?}"))?;
    let (second, _) = sim::article(&long, next).map_err(|e| format!("{e:?}"))?;
    second.save(format!("{dir}/article-page2.png"))?;
    let mut offset = 0;
    for n in 1..=3 {
        let (page, next) =
            sim::reader_page(sim::READER_SAMPLE, offset).map_err(|e| format!("{e:?}"))?;
        page.save(format!("{dir}/reader-{n}.png"))?;
        if next >= sim::READER_SAMPLE.len() {
            break;
        }
        offset = next;
    }
    Ok(())
}
