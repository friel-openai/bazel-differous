use bazel_differrous_proto::build::{QueryResult, Target};
use prost::Message;
use std::{env, fs, io::Cursor};

fn main() {
    let path = env::args()
        .nth(1)
        .expect("usage: decode_stream <path to streamed_proto>");
    let data = fs::read(&path).expect("read proto");
    let mut pos = 0usize;
    while pos < data.len() {
        if let Ok((msg, used)) = decode_message::<QueryResult>(&data[pos..]) {
            pos += used;
            for t in msg.target {
                dump_target(&t);
            }
            continue;
        }
        match decode_message::<Target>(&data[pos..]) {
            Ok((t, used)) => {
                pos += used;
                dump_target(&t);
            }
            Err(err) => {
                // If decoding fails, advance one byte to resync.
                pos += 1;
                let _ = err; // ignore detail
            }
        }
    }
}

fn decode_message<M: Message + Default>(buf: &[u8]) -> Result<(M, usize), prost::DecodeError> {
    let mut cursor = Cursor::new(buf);
    let len = prost::encoding::decode_varint(&mut cursor)? as usize;
    let start = cursor.position() as usize;
    let end = start + len;
    if end > buf.len() {
        return Err(prost::DecodeError::new("truncated"));
    }
    let msg = M::decode(&buf[start..end])?;
    Ok((msg, end))
}

fn dump_target(t: &Target) {
    if let Some(rule) = t.rule.as_ref() {
        println!("name: {}", rule.name);
        let cri: Vec<_> = rule
            .configured_rule_input
            .iter()
            .filter_map(|c| c.label.clone())
            .collect();
        println!("configured_rule_input: {:?}", cri);
        println!("rule_input: {:?}\n", rule.rule_input);
    }
}
