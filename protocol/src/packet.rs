use core::fmt;
use std::{cell, ops::{BitOrAssign, Shl, ShlAssign}, sync::Arc};


use indexmap::IndexSet;
use std::result::Result::Ok;
// use std::fmt::Error;
// use std::time::Instant;
// use std::{cell, fmt};
// use std::{fs::read, vec};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream, sync::Mutex,
};

extern crate stubs;
use stubs::{GOLRequest, GOLResponse, PacketParams, Params, ProcessSliceArgs};

use super::{
    coordinates::Coordinates, function_call::{self, CallType, FunctionCall}
};
// originally used standard hashset but doesnt have order
// index set retains order of insertion
// this increases decode time by about 30-40% but i believe it is a worthy tradeoff

pub const BYTE: usize = 8;
pub const BYTE_F64: f64 = 8.0;
pub const CURRENT_VERSION: u8 = 1;

const HEADER_SIZE_BYTES: usize = 40;
const VERSION: usize = 0;
const CALL_TYPE: usize = 0;
const FUNCTION_CALL: usize = 1;
const MESSAGE_ID: usize = 2;
const IMAGE_SIZE: usize = 4;
const THREADS: usize = 6;
const Y1: usize = 7;
const Y2: usize = 9;
const TURNS: usize = 11;
const IP_ADDRESS: usize = 15;
const LENGTH: usize = 35;
const CHECKSUM: usize = 38;

#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    Other(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "IO error: {}", e),
            DecodeError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub version: u8,
    pub call_type: CallType,
    pub fn_call_id: u8,
    pub msg_id: u16,
    pub image_size: u16,
	pub threads: u8,
    pub y1: u16,
    pub y2: u16,
	pub turns: u32,
	pub ip_address: String,
    pub length: u32,
    pub checksum: u16,
}

impl Header {
    pub fn new() -> Self {
        Self {
            version: 1,
            call_type: CallType::Default,
            fn_call_id: 0,
            msg_id: 0,
            image_size: 0,
			threads:0,
            y1:0,
            y2: 0,
			turns:0,
			ip_address: String::new(),
            length: 0,
            checksum: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Packet {
}


pub trait IsUINT {}
impl IsUINT for u16 {}
impl IsUINT for u32 {}
impl IsUINT for u64 {}

impl Packet {
    pub fn new() -> Self {
        Self{   

        }
    }

	fn decode_header_entry<T>(data: &[u8], entry: usize, end: usize) -> Option<T> 
	where 
		T: IsUINT + Default + BitOrAssign + From<u8> + Shl<u8, Output = T> + ShlAssign<u8>,
	{
		let mut buf: T = Default::default();
        for byte in &data[entry..=entry+end] {
            buf <<= 8;
            buf |= T::from(*byte);
        }
        
        return Some(buf);
	}

    fn decode_header(&mut self, data: &[u8]) -> Result<Header,DecodeError> {
        // println!("VERSION {:?}, CALLTYPE {:?}", data[(VERSION & 0xF0) >> 4], data[CALL_TYPE & 0xF] );
        let header = Header {
            version: (data[VERSION ] & 0xF0) >> 4,       // first 4 bits
            call_type: CallType::from(data[CALL_TYPE] & 0xF), // second 4 bits
            fn_call_id: data[FUNCTION_CALL], // second byte
            msg_id:	Self::decode_header_entry(data, MESSAGE_ID, 1).unwrap(),// 3rd & 4th byte
            image_size: Self::decode_header_entry(data, IMAGE_SIZE, 1).unwrap(), // 5th & 6th byte
			threads: data[THREADS], // 7th byte
            y1: Self::decode_header_entry(data, Y1, 1).unwrap(),
            y2: Self::decode_header_entry(data, Y2, 1).unwrap(),
			turns:  Self::decode_header_entry(data, TURNS, 3).unwrap(), // 8th -> 11th byte
			ip_address: {
				match String::from_utf8(data[IP_ADDRESS..IP_ADDRESS+19].to_vec()) {
					Ok(ip) => ip.trim_matches('\0').to_string(),
					Err(e) => return Err(DecodeError::Other(format!("Error decoding ip address in header to string: {}", e)))
				}
			}, // 12th -> 31st byte

            length: Self::decode_header_entry(data, LENGTH, 2).unwrap(),  // 32nd -> 34th byte
            checksum: Self::decode_header_entry(data, CHECKSUM, 1).unwrap(), // 35th & 36th byte
        };

		Ok(header)
    }

    fn decode_payload(
        &mut self,
        cells: &mut IndexSet<u32>,
        data: &[u8],
        coordinate_length: u32,
        offset: u32,
    ) {
        let mut buffer: u32 = 0;
        let mut bit_count = 7;

        // could borrow here
        let mask: u32 = Packet::generate_mask(coordinate_length);
        let limit = Packet::limit(coordinate_length);
        let coordinate_length_usize: usize = coordinate_length as usize;
        for byte in data {
            buffer |= (*byte as u32) << 31 - bit_count; // adds next byte to the buffer
            bit_count += BYTE;

            // n = coordinate_length
            // while there is no space to shift, process first n bits
            while bit_count >= limit {
                let extracted_value = (buffer & mask) >> offset; // get first n bits then shift to right hand side

                cells.insert(extracted_value);

                buffer <<= coordinate_length; // shift buffer to the right by n bits
                bit_count -= coordinate_length_usize; // decrease bit count to account for bits just extracted
            }
        }
    }

    pub async fn read_header(&mut self, client: &mut Arc<Mutex<TcpStream>>) -> Result<Header, DecodeError> {
        // initialses buffer for header read
        let mut header_buf =  vec![0u8; HEADER_SIZE_BYTES];
        header_buf.resize(HEADER_SIZE_BYTES, 0);
        // reads header by reading exactly HEADER_SIZE number of bytes
        
        let mut client_guard = client.lock().await;
        // peak until bytes are ready then read.

        match client_guard.read_exact(&mut header_buf).await {
            Ok(_) => {
               // If HEADER_SIZE_BYTES have been read, then decode the header
               return self.decode_header(&header_buf)
            }
            Err(e) => {
                println!("ERROR: {:?}", e);
                return Err(DecodeError::Other(format!(
                    "Failed to read header from port; err = {:?}",
                    e
                )))
            }
        }
    }

    pub async fn read_payload(&mut self, client:  &mut Arc<Mutex<TcpStream>>, header: Header) -> Result<GOLRequest, DecodeError> {

        let (coordinate_length, offset) =
		Packet::calc_coord_len_and_offset(header.image_size as u32);
            
        // initialse payload buffer to be read into
        let payload_size = header.length as usize - HEADER_SIZE_BYTES;
        let mut payload_buf = vec![0u8; payload_size];

        
        let mut client_guard = client.lock().await;
        
        // Read exact number of bytes for payload
        match client_guard.read_exact(&mut payload_buf).await {
            Ok(_) => {

                let params = Params {
                    incoming_ip_address: header.ip_address,
                    turns: header.turns,
                    threads: header.threads as u32,
                    image_size: header.image_size as u32,
                };

                let mut request = GOLRequest::new(params, header.length as usize);
                self.decode_payload(&mut request.alive_cells, &payload_buf, coordinate_length, offset);

                Ok(request)
            }
            Err(e) => {
                Err(DecodeError::Other(format!("Failed to read payload: {}", e)))
            }
        }
    }

    pub async fn read_gol_response_payload(&mut self, client: &mut Arc<Mutex<TcpStream>>, header: Header) -> Result<GOLResponse, DecodeError> {
        let (coordinate_length, offset) =
		Packet::calc_coord_len_and_offset(header.image_size as u32);
            
        // initialse payload buffer to be read into
        let payload_size = header.length as usize - HEADER_SIZE_BYTES;
        let mut payload_buf = vec![0u8; payload_size];

        
        let mut client_guard = client.lock().await;
        
        // Read exact number of bytes for payload
        match client_guard.read_exact(&mut payload_buf).await {
            Ok(_) => {


                let params = Params {
                    incoming_ip_address: header.ip_address,
                    turns: header.turns,
                    threads: header.threads as u32,
                    image_size: header.image_size as u32,
                };

                let length = header.length / coordinate_length;
                let mut response = GOLResponse::new(length as usize, header.turns, 0, false);

                // decode the payload
                self.decode_payload(&mut response.world, &payload_buf, coordinate_length, offset);

                response.alive_count = response.world.len() as u32;

                Ok(response)
            }
            Err(e) => {
                Err(DecodeError::Other(format!("Failed to read payload: {}", e)))
            }
        }

        // initialise IndexSet for cells
        // initialising hear to decouple from decode_payload function for easy future modification
        // is currently initialised after the stream to avoid allocation time in the case of failure as index set allocation takes O(n)
        
    }

    pub async fn read_to_worker_payload(&mut self, client:  &mut Arc<Mutex<TcpStream>>, header: Header) -> Result<ProcessSliceArgs, DecodeError> {
        let (coordinate_length, offset) =
		Packet::calc_coord_len_and_offset(header.image_size as u32);
            
        // initialse payload buffer to be read into
        let payload_size = header.length as usize - HEADER_SIZE_BYTES;
        let mut payload_buf = vec![0u8; payload_size];
        let mut client_guard = client.lock().await;
        match client_guard.read_exact(&mut payload_buf).await {
            Ok(_) => {


                let cells = Arc::new(Mutex::new(IndexSet::with_capacity(((header.length - HEADER_SIZE_BYTES as u32) / (coordinate_length/8)) as usize)));
       
        
        // initialise IndexSet for cells
        // initialising hear to decouple from decode_payload function for easy future modification
        // is currently initialised after the stream to avoid allocation time in the case of failure as index set allocation takes O(n)
            let request = ProcessSliceArgs {
                image_size: header.image_size as u32,
                y1: header.y1,
                y2: header.y2,
                alive_cells: cells,
                threads: header.threads,

            };

            // decode the payload
            let mut alive_cells_guard = request.alive_cells.lock().await;
            self.decode_payload(&mut alive_cells_guard, &payload_buf, coordinate_length, offset);
            drop(alive_cells_guard);
            Ok(request)
            }
            Err(e) => {
                Err(DecodeError::Other(format!("Failed to read payload: {}", e)))
            }


        }
    }
   

    /***********************************
     ************ ENCODINGS ************
     ***********************************/

    pub async fn encode_payload_from_set(
        &self,
        cells: Arc<Mutex<IndexSet<u32>>>,
        coordinate_length: usize,
    ) -> Vec<u8> {
        let mut buffer: u32 = 0;
        let mut bit_count: usize = coordinate_length - 1;
        let mask: u32 = 0xFF000000;
        let cells_guard = cells.lock().await;
        let capacity = cells_guard.len() as f64 * (coordinate_length as f64 / BYTE_F64);
        let mut data = Vec::with_capacity(capacity as usize);
        for cell in cells_guard.iter() {
            buffer |= cell << 31 - bit_count;
            bit_count += coordinate_length;
            while bit_count >= 32 {
                let byte = buffer & mask;
                bit_count -= BYTE;
                buffer <<= BYTE;

                data.push((byte >> 24) as u8);
            }
        }
        while buffer != 0 {
            let byte = (buffer & 0xFF000000) >> 24;
            data.push(byte as u8);
            buffer <<= BYTE;
        }
        data
    }

    /// ### General Info
    /// Encodes header while calculating checksum.
    /// Computed checksum here to avoid having to pass around and get out all this data in a separate function.
    ///
    /// Note that `message length` = `payload length` + `header length`
    /// 
    /// ## Header Layout
    /// 
    /// ```
    /// LSB                                                                                                       MSB
    ///   1 2 3 4   5 6 7 8   1 2 3 4 5 6 7 8   1 2 3 4 5 6 7 8  1 2 3 4 5 6 7 8   1 2 3 4 5 6 7 8  1 2 3 4 5 6 7 8
    /// +---------+---------+-----------------+----------------------------------+----------------------------------+ 
    /// | Version |  Type   |     Fn Call     |            Message ID            |            Image Size            |
    /// +---------+---------+-----------------+----------------------------------+----------------------------------+
    /// +-------------------+-----------------------------------+-----------------------------------+
    /// |      threads      |                y1                 |                y2                 |
    /// +-------------------+-----------------------------------+-----------------------------------+
    /// +------------------------------------------------------------------------+
    /// |                                     turns                              |
    /// +------------------------------------------------------------------------+
    /// +-----------------------------------------------------------------------------------------------------------+
    /// |                                            ip address 20 bytes                                            |
    /// +-----------------------------------------------------------------------------------------------------------+
    /// +----------------------------------------------------+
    /// |                       Length                       |
    /// +----------------------------------------------------+
    /// +----------------------------------+
    /// |      16-bit Checksum CRC-16      |
    /// +----------------------------------+
    /// +-----------------------------------------------------------------------------------------------------------+
    /// |                                  Alive Cells = Length - 40 bytes long                                     |
    /// +-----------------------------------------------------------------------------------------------------------+
    /// ```
    /// 
    pub fn encode_header(&self, params: PacketParams, message_length: u32, sum: &mut u32) -> Vec<u8> {
        let mut data = Vec::with_capacity(HEADER_SIZE_BYTES);
        let first = CURRENT_VERSION << 4 | params.call_type;
        data.push(first);
        data.push(params.fn_call_id);
        data.push_u16_to_u8s(params.msg_id);
        data.push_u16_to_u8s(params.image_size);
        data.push(params.threads);
        data.push_u16_to_u8s(params.y1);
        data.push_u16_to_u8s(params.y2);
        data.push_u32_to_u8s(params.turns);
        data.push_string_to_u8s(params.sender_ip.clone());
        data.push_u24_to_u8s(message_length);

        self.ones_complement_sum(sum, CURRENT_VERSION as u16);
        self.ones_complement_sum(sum, params.fn_call_id as u16);
        self.ones_complement_sum(sum, params.msg_id);
        self.ones_complement_sum(sum, params.image_size);
        self.ones_complement_sum(sum, params.threads as u16);
        self.ones_complement_sum(sum, params.y1);
        self.ones_complement_sum(sum, params.y2);
        self.ones_complement_sum(sum, (params.turns >> 24) as u16);
        self.ones_complement_sum(sum, ((params.turns >> 16) & 0xFF) as u16);
        self.ones_complement_sum(sum, ((params.turns >> 8) & 0xFF) as u16);
        self.ones_complement_sum(sum, (params.turns & 0xFF) as u16);
        for byte in params.sender_ip.bytes() {
            self.ones_complement_sum(sum, byte as u16);
        }
        self.ones_complement_sum(sum, (message_length >> 16) as u16);
        self.ones_complement_sum(sum, message_length as u16);
        return data;
    }

    /// Encodes cells to bytes and writes them to the stream  
    ///
    /// Use of `write_all` ensures all bytes are written or there is an error before the function returns.
    pub async fn write_data(
        &self,
        client: &mut Arc<Mutex<TcpStream>>,
        params: PacketParams,
        cells: Arc<Mutex<IndexSet<u32>>>,
    ) -> Result<(), DecodeError> {
        let (coordinate_length, _) =
            Packet::calc_coord_len_and_offset(params.image_size as u32);

        // payload_length is length of alivecells in bytes + header size in bytes
        let payload = self.encode_payload_from_set(cells, coordinate_length as usize).await;
        let message_length = payload.len() as u32 + HEADER_SIZE_BYTES as u32;
        let mut sum: u32 = 0;

        let mut header = self.encode_header(params, message_length, &mut sum);
        
        // sum up contents of payload
        // could combine into encoding but doesnt increase runtime apart from increasing constant factor
        // combining into encoding would just increase complexity for a reduction in the constant factor of only ≈1
        for i in 0..payload.len() / 2 {
            let word = ((payload[i * 2] as u16) << 8) | (payload[i * 2 + 1] as u16);
            self.ones_complement_sum(&mut sum, word);
        }
        // handle odd-length payload
        if payload.len() % 2 == 1 {
            self.ones_complement_sum(&mut sum, (payload[payload.len() - 1] as u16) << 8);
        }

        // bitwise ones compliment
        let checksum = !(sum as u16);

        // append checksum to last 2 bytes of header
        header.push_u16_to_u8s(checksum);

        // WILL USE WHEN WORKING => USING ASSERT FOR NOW TO PANIC AND THROW ERROR RATHER THAN IMPLEMENT SPECIFIC HANDLING
        // if header.len() != HEADER_SIZE_BYTES {
        //     return Err(DecodeError::Other(format!(
        //         "Header is the wrong length, should be {:?} bytes long but is {:?} bytes long instead. \n
        //         The header is: {:?} \n",
        //         HEADER_SIZE_BYTES, header.len(), header
        //     )));
        // }

        assert_eq!(HEADER_SIZE_BYTES, header.len(), 
                "Header is the wrong length, should be {:?} bytes long but is {:?} bytes long instead. \n
                The header is: {:?} \n",
                HEADER_SIZE_BYTES, header.len(), header
            );

        // write HEADER_SIZE number of bites returning error if there is one
        let mut client_guard = client.lock().await;
        match client_guard.write_all(&header).await {
            Ok(_) => {

                // write payload length number of bites if header returns successfully
                return match client_guard.write_all(&payload).await {
                    Ok(_) =>{ 
                        Ok(())
                    },
                    Err(e) => {
                        return Err(DecodeError::Other(format!(
                            "Failed to write payload from port; err = {:?}",
                            e
                        )));
                    }
                };
            }
            Err(e) => {
                return Err(DecodeError::Other(format!(
                    "Failed to write header from port; err = {:?}",
                    e
                )));
            }
        };
    }

    pub async fn write_gol_response(
        &self,
        client: &mut Arc<Mutex<TcpStream>>,
        params: PacketParams,
        cells: Arc<IndexSet<u32>>,
    ){

    }



    fn ones_complement_sum(&self, sum: &mut u32, word: u16) {
        *sum += word as u32;
        if *sum > 0xFFFF {
            *sum = (*sum & 0xFFFF) + 1;
        }
    }

    
}

trait NumbersAsBytes {
    fn push_u16_to_u8s(&mut self, num: u16);
    fn push_u24_to_u8s(&mut self, num: u32);
    fn push_u32_to_u8s(&mut self, num: u32);
    fn push_string_to_u8s(&mut self, string: String);
}

impl NumbersAsBytes for Vec<u8> {
    fn push_u16_to_u8s(&mut self, num: u16) {
        self.push((num >> 8) as u8);
        self.push(num as u8);
    }

    /// pushes left hand 3 bytes to stack
    fn push_u24_to_u8s(&mut self, num: u32) {
        self.push((num >> 16) as u8);
        self.push((num >> 8) as u8);
        self.push(num as u8);
    }

    fn push_u32_to_u8s(&mut self, num: u32) {
        self.push((num >> 24) as u8);
        self.push((num >> 16) as u8);
        self.push((num >> 8) as u8);
        self.push(num as u8);
    }

    fn push_string_to_u8s(&mut self, string: String) {
        let bytes = string.as_bytes();
        let mut index = 20 - bytes.len() -1;
        let mut buf: Vec<u8> = vec![0; 20];
        for byte in bytes {
            buf[index] = *byte;
            index+=1;
        }
        self.append(&mut buf);
    }
}


impl Coordinates for Packet {
	fn calc_coord_len_and_offset(image_size: u32) -> (u32, u32) {
        let coordinate_length = || -> u32 {
            let mask: u32 = 1;
            let mut size = 0;
            for i in 0..32 as u32 {
                if image_size & (mask << i) > 0 {
                    size = i;
                }
            }
            return size * 2;
        }();
        let offset = 32 - coordinate_length;
        return (coordinate_length, offset);
    }

	fn generate_mask(coordinate_length: u32) -> u32 {
        if coordinate_length > 32 {
            panic!("coordinate length must be less than or equal to 32");
        }
        let mask = !0u32;
        mask << (32 - coordinate_length)
    }

	fn limit(coordinate_length: u32) -> usize {
        if coordinate_length > 24 {
            32
        } else if coordinate_length > 16 {
            24
        } else if coordinate_length > 8 {
            16
        } else {
            8
        }
    }
}
