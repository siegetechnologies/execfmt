use std::io::prelude::*;
use std::io;
use std::fmt;
use std::error;
use byteorder;
use byteorder::ReadBytesExt;
use elf::types;
use std::collections::HashMap;
use {Error, Section, Object};

macro_rules! read_u8 {
    ($data:ident, $io:ident) => (
        $io.read_u8()
    );
}

macro_rules! read_u16 {
    ($data:ident, $io:ident) => (
        match $data {
            types::ELFDATA2LSB => { $io.read_u16::<byteorder::LittleEndian>() },
            types::ELFDATA2MSB => { $io.read_u16::<byteorder::BigEndian>()},
            _ => { try!(Err(Error::from("invalid endianness"))) },
        }
    );
}

macro_rules! read_u32 {
    ($data:ident, $io:ident) => (
        match $data {
            types::ELFDATA2LSB => { $io.read_u32::<byteorder::LittleEndian>() },
            types::ELFDATA2MSB => { $io.read_u32::<byteorder::BigEndian>()},
            _ => { try!(Err(Error::from("invalid endianness"))) },
        }
    );
}

macro_rules! read_u64 {
    ($data:ident, $io:ident) => (
        match $data {
            types::ELFDATA2LSB => { $io.read_u64::<byteorder::LittleEndian>() },
            types::ELFDATA2MSB => { $io.read_u64::<byteorder::BigEndian>()},
            _ => { try!(Err(Error::from("invalid endianness"))) },
        }
    );
}

fn get_elf_string(data: &Vec<u8>, start: usize) -> String {
    let mut end = 0usize;
    for i in start..data.len() {
        if data[i] == 0u8 {
            end = i;
            break;
        }
    }

    let mut ret = String::with_capacity(end - start);
    for i in start..end {
        ret.push(data[i] as char);
    }

    ret
}

pub struct File {
    pub hdr: types::FileHeader,
    pub sections: HashMap<String, Section>,
    pub symbols: HashMap<String, u64>,
}

impl File {
    #[allow(unused_variables,unused_assignments)]
    pub fn parse<R: io::Read + io::Seek>(r: &mut R) -> Result<File, Box<error::Error>> {
        try!(r.seek(io::SeekFrom::Start(0)));
        let mut eident = [0u8; types::EI_NIDENT];
        try!(r.read(&mut eident));

        if eident[0..4] != types::ELFMAG {
            try!(Err(Error::from("invalid magic number")));
        }

        let class = types::Class(eident[types::EI_CLASS]);
        let data = types::Data(eident[types::EI_DATA]);
        let os_abi = types::OsAbi(eident[types::EI_OSABI]);
        let abi_version = eident[types::EI_ABIVERSION];

        let elf_type = types::Type(try!(read_u16!(data, r)));
        let machine = types::Machine(try!(read_u16!(data, r)));
        let version = types::Version(try!(read_u32!(data, r)));

        let entry: u64;
        let phoff: u64;
        let shoff: u64;

        match class {
            types::ELFCLASS32 => {
                entry = try!(read_u32!(data, r)) as u64;
                phoff = try!(read_u32!(data, r)) as u64;
                shoff = try!(read_u32!(data, r)) as u64;
            }
            types::ELFCLASS64 => {
                entry = try!(read_u64!(data, r));
                phoff = try!(read_u64!(data, r));
                shoff = try!(read_u64!(data, r));
            }
            _ => return Err(Box::new(Error::from("invalid class"))),
        }

        let flags = try!(read_u32!(data, r));
        let ehsize = try!(read_u16!(data, r));
        let phentsize = try!(read_u16!(data, r));
        let phnum = try!(read_u16!(data, r));
        let shentsize = try!(read_u16!(data, r));
        let shnum = try!(read_u16!(data, r));
        let shstrndx = try!(read_u16!(data, r));

        let mut sections = HashMap::new();
        let mut sections_lst = Vec::new();
        let mut sections_data = Vec::new();

        let mut name_idxs = Vec::new();
        try!(r.seek(io::SeekFrom::Start(shoff)));

        for _ in 0..shnum {
            let name = String::new();
            let shtype: types::SectionType;
            let flags: types::SectionFlag;
            let addr: u64;
            let offset: u64;
            let size: u64;
            let link: u32;
            let info: u32;
            let addralign: u64;
            let entsize: u64;

            name_idxs.push(try!(read_u32!(data, r)));
            shtype = types::SectionType(try!(read_u32!(data, r)));
            match class {
                types::ELFCLASS32 => {
                    flags = types::SectionFlag(try!(read_u32!(data, r)) as u64);
                    addr = try!(read_u32!(data, r)) as u64;
                    offset = try!(read_u32!(data, r)) as u64;
                    size = try!(read_u32!(data, r)) as u64;
                    link = try!(read_u32!(data, r));
                    info = try!(read_u32!(data, r));
                    addralign = try!(read_u32!(data, r)) as u64;
                    entsize = try!(read_u32!(data, r)) as u64;
                }
                types::ELFCLASS64 => {
                    flags = types::SectionFlag(try!(read_u64!(data, r)));
                    addr = try!(read_u64!(data, r));
                    offset = try!(read_u64!(data, r));
                    size = try!(read_u64!(data, r));
                    link = try!(read_u32!(data, r));
                    info = try!(read_u32!(data, r));
                    addralign = try!(read_u64!(data, r));
                    entsize = try!(read_u64!(data, r));
                }
                _ => unreachable!(),
            }

            sections_lst.push(types::SectionHeader {
                name: name,
                shtype: shtype,
                flags: flags,
                addr: addr,
                offset: offset,
                size: size,
                link: link,
                info: info,
                addralign: addralign,
                entsize: entsize,
            });
        }

        for i in 0..shnum {
            let off = sections_lst[i as usize].offset;
            let size = sections_lst[i as usize].size;
            try!(r.seek(io::SeekFrom::Start(off)));
            let data: Vec<u8> = io::Read::by_ref(r).bytes().map(|x| x.unwrap()).take(size as usize).collect();
            sections_data.push(data);
        }

        let mut symbols = HashMap::new();

        for (i, section) in sections_lst.iter().enumerate() {
            if section.shtype == types::SHT_SYMTAB {
                let mut cur = io::Cursor::new(sections_data[i].as_slice());
                for i in 0..(section.size / section.entsize) {
                    try!(cur.seek(io::SeekFrom::Start(i * section.entsize)));
                    let sym_name;
                    let sym_addr;
                    match class {
                        types::ELFCLASS32 => {
                            sym_name = try!(read_u32!(data, cur));
                            sym_addr = try!(read_u32!(data, cur)) as u64;
                        }
                        types::ELFCLASS64 => {
                            sym_name = try!(read_u32!(data, cur));
                            let _ = try!(read_u8!(data, cur));
                            let _ = try!(read_u8!(data, cur));
                            let _ = try!(read_u16!(data, cur));
                            sym_addr = try!(read_u64!(data, cur));
                        }
                        _ => unreachable!(),
                    }
                    symbols.insert(get_elf_string(&sections_data[section.link as usize], sym_name as usize), sym_addr);
                }
            }
        }

        for i in 0..shnum {
            sections_lst[i as usize].name = get_elf_string(&sections_data[shstrndx as usize], name_idxs[i as usize] as usize);
        }

        for (hdr, data) in sections_lst.into_iter().zip(sections_data.into_iter()) {
            sections.insert(hdr.name.clone(), Section { name: hdr.name, addr: hdr.addr, offset: hdr.offset, size: hdr.size, data: data });
        }

        let x = File {
            hdr: types::FileHeader {
                class: class,
                data: data,
                version: version,
                os_abi: os_abi,
                abi_version: abi_version,
                elf_type: elf_type,
                machine: machine,
                entrypoint: entry,
            },
            sections: sections,
            symbols: symbols,
        };
        Ok(x)
    }

    pub fn sections(&self) -> &HashMap<String, Section> {
        &self.sections
    }
    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.symbols
    }
}

impl fmt::Display for File {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        try!(writeln!(f, "ELF file"));
        try!(write!(f, "{}", self.hdr));
        try!(writeln!(f, "ELF sections"));
        for section in self.sections.values() {
            try!(write!(f, "{:?}", section));
        }
        try!(writeln!(f, "ELF symbols"));
        let mut x: Vec<&String> = self.symbols.keys().collect();
        x.sort();
        for key in x.into_iter() {
            try!(writeln!(f, "{}: {:#x}", key, self.symbols[key]));
        }
        Ok(())
    }
}

impl Object for File {
    fn arch(&self) -> ::Arch {
        let endian = match self.hdr.data {
            types::ELFDATA2LSB => ::Endianness::Little,
            types::ELFDATA2MSB => ::Endianness::Big,
            _ => return ::Arch::Unknown,
        };
        match self.hdr.machine {
            types::EM_386 => ::Arch::X86(::Width::W32),
            types::EM_X86_64 => ::Arch::X86(::Width::W64),
            types::EM_PPC => ::Arch::PPC(::Width::W32, endian),
            types::EM_PPC64 => ::Arch::PPC(::Width::W64, endian),
            types::EM_ARM => ::Arch::ARM(::Width::W32, endian, ::ARMMode::ARM, ::ARMType::ARM),
            types::EM_AARCH64 => ::Arch::ARM(::Width::W64, endian, ::ARMMode::ARM, ::ARMType::ARM),
            _ => ::Arch::Unknown,
        }
    }
    fn get_section(&self, name: &str) -> Option<&Section> {
        self.sections.get(name)
    }
}
