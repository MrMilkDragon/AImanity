use std::{
    env,
    error::Error,
    io::{self, Read},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let width: usize = args.next().ok_or("missing width")?.parse()?;
    let height: usize = args.next().ok_or("missing height")?.parse()?;
    let mut frame = Vec::new();
    io::stdin().read_to_end(&mut frame)?;
    if width == 0 || width.checked_mul(height) != Some(frame.len()) {
        return Err("unexpected grayscale frame size".into());
    }

    let left = width * 35 / 100;
    let center = left..left + width * 30 / 100;
    let bottom = (height * 97 / 100 + 1).min(height);
    let mut start = height / 2;
    let mut best = start..start;
    for y in start..=bottom {
        if y < bottom {
            let row = &frame[y * width..(y + 1) * width];
            if row[center.clone()]
                .iter()
                .filter(|&&pixel| pixel == 0)
                .count()
                * 25
                > width
            {
                continue;
            }
        }
        if y - start > best.len() {
            best = start..y;
        }
        start = y + 1;
    }
    if best.len().saturating_sub(1) * 1000 <= height * 18 {
        return Err("no centered action button in language frame".into());
    }
    println!("{}", (best.start + best.end - 1) / 2);
    Ok(())
}
