use crate::error::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Largest frame (header included) we accept from the wire.
pub const MAX_PACKET_LEN: u32 = 64 * 1024 * 1024;

const HEADER_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    InitCommandRequest {
        guid: [u8; 16],
        name: String,
        version: u32,
    },
    InitCommandAck {
        conn_number: u32,
        guid: [u8; 16],
        name: String,
        version: u32,
    },
    InitEventRequest {
        conn_number: u32,
    },
    InitEventAck,
    InitFail {
        reason: u32,
    },
    CmdRequest {
        data_phase: u32,
        op: u16,
        tid: u32,
        params: Vec<u32>,
    },
    CmdResponse {
        code: u16,
        tid: u32,
        params: Vec<u32>,
    },
    Event {
        code: u16,
        tid: u32,
        params: Vec<u32>,
    },
    StartData {
        tid: u32,
        total_len: u64,
    },
    Data {
        tid: u32,
        payload: Vec<u8>,
    },
    EndData {
        tid: u32,
        payload: Vec<u8>,
    },
    Cancel {
        tid: u32,
    },
    ProbeRequest,
    ProbeResponse,
}

pub(crate) fn encode_utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Decodes a NUL-terminated UTF-16LE string; returns it and the bytes consumed
/// (terminator included).
pub(crate) fn decode_utf16z(b: &[u8]) -> Result<(String, usize), Error> {
    let mut units = Vec::new();
    for (i, &c) in b.as_chunks::<2>().0.iter().enumerate() {
        let u = u16::from_le_bytes(c);
        if u == 0 {
            let s = String::from_utf16(&units)
                .map_err(|_| Error::Protocol("invalid UTF-16 string".into()))?;
            return Ok((s, (i + 1) * 2));
        }
        units.push(u);
    }
    Err(Error::Protocol("unterminated UTF-16 string".into()))
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.0.len() < n {
            return Err(Error::Protocol("packet truncated".into()));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn guid(&mut self) -> Result<[u8; 16], Error> {
        Ok(self.take(16)?.try_into().unwrap())
    }
    fn string(&mut self) -> Result<String, Error> {
        let (s, n) = decode_utf16z(self.0)?;
        self.0 = &self.0[n..];
        Ok(s)
    }
    fn params(&mut self) -> Result<Vec<u32>, Error> {
        if !self.0.len().is_multiple_of(4) {
            return Err(Error::Protocol(
                "parameter bytes not a multiple of 4".into(),
            ));
        }
        let mut v = Vec::with_capacity(self.0.len() / 4);
        while !self.0.is_empty() {
            v.push(self.u32()?);
        }
        Ok(v)
    }
    fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0).to_vec()
    }
}

impl Packet {
    fn type_number(&self) -> u32 {
        match self {
            Packet::InitCommandRequest { .. } => 1,
            Packet::InitCommandAck { .. } => 2,
            Packet::InitEventRequest { .. } => 3,
            Packet::InitEventAck => 4,
            Packet::InitFail { .. } => 5,
            Packet::CmdRequest { .. } => 6,
            Packet::CmdResponse { .. } => 7,
            Packet::Event { .. } => 8,
            Packet::StartData { .. } => 9,
            Packet::Data { .. } => 10,
            Packet::Cancel { .. } => 11,
            Packet::EndData { .. } => 12,
            Packet::ProbeRequest => 13,
            Packet::ProbeResponse => 14,
        }
    }

    /// Encodes the full frame: `[len u32][type u32][payload]`, little-endian.
    pub fn encode(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            Packet::InitCommandRequest {
                guid,
                name,
                version,
            } => {
                p.extend_from_slice(guid);
                p.extend(encode_utf16z(name));
                p.extend(version.to_le_bytes());
            }
            Packet::InitCommandAck {
                conn_number,
                guid,
                name,
                version,
            } => {
                p.extend(conn_number.to_le_bytes());
                p.extend_from_slice(guid);
                p.extend(encode_utf16z(name));
                p.extend(version.to_le_bytes());
            }
            Packet::InitEventRequest { conn_number } => p.extend(conn_number.to_le_bytes()),
            Packet::InitFail { reason } => p.extend(reason.to_le_bytes()),
            Packet::CmdRequest {
                data_phase,
                op,
                tid,
                params,
            } => {
                p.extend(data_phase.to_le_bytes());
                p.extend(op.to_le_bytes());
                p.extend(tid.to_le_bytes());
                params.iter().for_each(|x| p.extend(x.to_le_bytes()));
            }
            Packet::CmdResponse { code, tid, params } | Packet::Event { code, tid, params } => {
                p.extend(code.to_le_bytes());
                p.extend(tid.to_le_bytes());
                params.iter().for_each(|x| p.extend(x.to_le_bytes()));
            }
            Packet::StartData { tid, total_len } => {
                p.extend(tid.to_le_bytes());
                p.extend(total_len.to_le_bytes());
            }
            Packet::Data { tid, payload } | Packet::EndData { tid, payload } => {
                p.extend(tid.to_le_bytes());
                p.extend_from_slice(payload);
            }
            Packet::Cancel { tid } => p.extend(tid.to_le_bytes()),
            Packet::InitEventAck | Packet::ProbeRequest | Packet::ProbeResponse => {}
        }
        let mut out = Vec::with_capacity(HEADER_LEN + p.len());
        out.extend(((HEADER_LEN + p.len()) as u32).to_le_bytes());
        out.extend(self.type_number().to_le_bytes());
        out.extend(p);
        out
    }

    /// Decodes a full frame including its 8-byte header.
    pub fn decode(frame: &[u8]) -> Result<Packet, Error> {
        if frame.len() < HEADER_LEN {
            return Err(Error::Protocol("frame shorter than header".into()));
        }
        let len = u32::from_le_bytes(frame[0..4].try_into().unwrap()) as usize;
        if len != frame.len() {
            return Err(Error::Protocol(format!(
                "frame length {} does not match header length {}",
                frame.len(),
                len
            )));
        }
        let ty = u32::from_le_bytes(frame[4..8].try_into().unwrap());
        let mut r = Reader(&frame[HEADER_LEN..]);
        let pkt = match ty {
            1 => Packet::InitCommandRequest {
                guid: r.guid()?,
                name: r.string()?,
                version: r.u32()?,
            },
            2 => Packet::InitCommandAck {
                conn_number: r.u32()?,
                guid: r.guid()?,
                name: r.string()?,
                version: r.u32()?,
            },
            3 => Packet::InitEventRequest {
                conn_number: r.u32()?,
            },
            4 => Packet::InitEventAck,
            5 => Packet::InitFail { reason: r.u32()? },
            6 => Packet::CmdRequest {
                data_phase: r.u32()?,
                op: r.u16()?,
                tid: r.u32()?,
                params: r.params()?,
            },
            7 => Packet::CmdResponse {
                code: r.u16()?,
                tid: r.u32()?,
                params: r.params()?,
            },
            8 => Packet::Event {
                code: r.u16()?,
                tid: r.u32()?,
                params: r.params()?,
            },
            9 => Packet::StartData {
                tid: r.u32()?,
                total_len: r.u64()?,
            },
            10 => Packet::Data {
                tid: r.u32()?,
                payload: r.rest(),
            },
            11 => Packet::Cancel { tid: r.u32()? },
            12 => Packet::EndData {
                tid: r.u32()?,
                payload: r.rest(),
            },
            13 => Packet::ProbeRequest,
            14 => Packet::ProbeResponse,
            other => return Err(Error::Protocol(format!("unknown packet type {other}"))),
        };
        Ok(pkt)
    }
}

/// Reads one frame (header included) from the stream.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>, Error> {
    let mut header = [0u8; HEADER_LEN];
    r.read_exact(&mut header).await?;
    let len = u32::from_le_bytes(header[0..4].try_into().unwrap());
    if len < HEADER_LEN as u32 {
        return Err(Error::Protocol(format!(
            "frame length {len} below header size"
        )));
    }
    if len > MAX_PACKET_LEN {
        return Err(Error::Protocol(format!(
            "frame length {len} exceeds maximum"
        )));
    }
    let mut frame = vec![0u8; len as usize];
    frame[..HEADER_LEN].copy_from_slice(&header);
    r.read_exact(&mut frame[HEADER_LEN..]).await?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_init_command_request() {
        let p = Packet::InitCommandRequest {
            guid: [0x11; 16],
            name: "Rimix".into(),
            version: 0x0001_0000,
        };
        let mut want = vec![0x28, 0, 0, 0, 1, 0, 0, 0];
        want.extend([0x11; 16]);
        want.extend([0x52, 0, 0x69, 0, 0x6D, 0, 0x69, 0, 0x78, 0, 0, 0]);
        want.extend([0, 0, 1, 0]);
        assert_eq!(p.encode(), want);
    }

    #[test]
    fn decodes_init_command_ack() {
        let mut payload = Vec::new();
        payload.extend(7u32.to_le_bytes());
        payload.extend([0x22; 16]);
        payload.extend(encode_utf16z("ILCE-6700"));
        payload.extend(0x0001_0000u32.to_le_bytes());
        let mut frame = ((8 + payload.len()) as u32).to_le_bytes().to_vec();
        frame.extend(2u32.to_le_bytes());
        frame.extend(payload);
        assert_eq!(
            Packet::decode(&frame).unwrap(),
            Packet::InitCommandAck {
                conn_number: 7,
                guid: [0x22; 16],
                name: "ILCE-6700".into(),
                version: 0x0001_0000
            }
        );
    }

    #[test]
    fn cmd_request_roundtrip() {
        let p = Packet::CmdRequest {
            data_phase: 1,
            op: 0x1002,
            tid: 1,
            params: vec![7],
        };
        let b = p.encode();
        assert_eq!(b.len(), 22);
        assert_eq!(Packet::decode(&b).unwrap(), p);
    }

    #[test]
    fn data_packets_roundtrip() {
        for p in [
            Packet::StartData {
                tid: 3,
                total_len: 5,
            },
            Packet::Data {
                tid: 3,
                payload: vec![1, 2, 3],
            },
            Packet::EndData {
                tid: 3,
                payload: vec![4, 5],
            },
        ] {
            assert_eq!(Packet::decode(&p.encode()).unwrap(), p);
        }
    }

    #[test]
    fn rejects_short_frame() {
        assert!(matches!(
            Packet::decode(&[1, 2, 3]),
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn read_frame_rejects_oversized_length() {
        let mut r: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 6, 0, 0, 0];
        assert!(matches!(read_frame(&mut r).await, Err(Error::Protocol(_))));
    }

    #[tokio::test]
    async fn read_frame_rejects_tiny_length() {
        let mut r: &[u8] = &[4, 0, 0, 0, 6, 0, 0, 0];
        assert!(read_frame(&mut r).await.is_err());
    }

    #[tokio::test]
    async fn read_frame_returns_whole_frame() {
        let b = Packet::Cancel { tid: 9 }.encode();
        let mut r: &[u8] = &b;
        assert_eq!(read_frame(&mut r).await.unwrap(), b);
    }
}
