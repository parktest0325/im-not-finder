//! Windows delayed-rendering ("promised files") drag source.
//!
//! On drag we hand the OS a custom `IDataObject` that advertises a
//! `FileGroupDescriptorW` (file names) and only fetches each file's bytes when
//! the drop target asks for `FileContents` — i.e. at drop time.
//!
//! - Folder enumeration is LAZY (only when the descriptor is requested) and uses
//!   the transport's fast `walk_files`, so the drag itself starts instantly.
//! - On the first content request we download EVERYTHING in parallel and cache
//!   it, so a drop of many files isn't N sequential round-trips.

#![cfg(windows)]

use crate::transport::Transport;
use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::{Arc, OnceLock};
use windows::core::{implement, w, BOOL, HRESULT};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::{
    IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumFORMATETC_Impl, IEnumSTATDATA,
    IStream, FORMATETC, STGMEDIUM, STGMEDIUM_0, DATADIR_GET, DVASPECT_CONTENT, TYMED_HGLOBAL,
    TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, DROPEFFECT, DROPEFFECT_COPY,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{SHCreateMemStream, FILEDESCRIPTORW};
use std::sync::Mutex;

const S_OK: HRESULT = HRESULT(0);
const S_FALSE: HRESULT = HRESULT(1);
const E_NOTIMPL: HRESULT = HRESULT(0x8000_4001u32 as i32);
const DV_E_FORMATETC: HRESULT = HRESULT(0x8004_0064u32 as i32);
const OLE_E_ADVISENOTSUPPORTED: HRESULT = HRESULT(0x8004_0003u32 as i32);
const DRAGDROP_S_DROP: HRESULT = HRESULT(0x0004_0100);
const DRAGDROP_S_CANCEL: HRESULT = HRESULT(0x0004_0101);
const DRAGDROP_S_USEDEFAULTCURSORS: HRESULT = HRESULT(0x0004_0102);
const FD_FILESIZE: u32 = 0x40;
const FD_PROGRESSUI: u32 = 0x4000;
const MK_LBUTTON: u32 = 0x0001;

fn werr(h: HRESULT) -> windows::core::Error {
    windows::core::Error::from_hresult(h)
}

/// A top-level dragged item (file or folder root) as picked in the file tree.
pub struct Item {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// One concrete file in the flattened descriptor.
struct Entry {
    name: Vec<u16>, // relative, `\`-separated, UTF-16 (no NUL)
    path: String,   // absolute remote path
    size: u64,
}

fn utf16_backslash(s: &str) -> Vec<u16> {
    s.replace('/', "\\").encode_utf16().collect()
}

async fn read_all(t: &Arc<dyn Transport>, path: &str) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    let chunk = 4 * 1024 * 1024u64;
    let mut off = 0u64;
    loop {
        let part = t.read_chunk(path, off, chunk).await?;
        let n = part.len() as u64;
        out.extend_from_slice(&part);
        if n < chunk {
            break;
        }
        off += n;
    }
    Ok(out)
}

/// Flatten the top-level items into concrete files (folders walked recursively).
async fn flatten(t: &Arc<dyn Transport>, items: &[Item]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for item in items {
        if item.is_dir {
            if let Ok(files) = t.walk_files(&item.path).await {
                let base = item.path.trim_end_matches('/');
                for (fp, size) in files {
                    let tail = fp.strip_prefix(base).unwrap_or(&fp).trim_start_matches('/');
                    let rel = if tail.is_empty() {
                        item.name.clone()
                    } else {
                        format!("{}/{}", item.name, tail)
                    };
                    entries.push(Entry {
                        name: utf16_backslash(&rel),
                        path: fp,
                        size,
                    });
                }
            }
        } else {
            entries.push(Entry {
                name: utf16_backslash(&item.name),
                path: item.path.clone(),
                size: item.size,
            });
        }
    }
    entries
}

unsafe fn build_descriptor(entries: &[Entry]) -> windows::core::Result<HGLOBAL> {
    let n = entries.len();
    let fd_size = std::mem::size_of::<FILEDESCRIPTORW>();
    let total = 4 + n * fd_size;
    let hg = GlobalAlloc(GMEM_MOVEABLE, total)?;
    let base = GlobalLock(hg) as *mut u8;
    if base.is_null() {
        return Err(werr(DV_E_FORMATETC));
    }
    ptr::write_unaligned(base as *mut u32, n as u32);
    let fd_base = base.add(4) as *mut FILEDESCRIPTORW;
    for (i, e) in entries.iter().enumerate() {
        let mut cfn = [0u16; 260];
        let len = e.name.len().min(259);
        cfn[..len].copy_from_slice(&e.name[..len]);
        let mut fd = FILEDESCRIPTORW::default();
        fd.dwFlags = if e.size > 0 {
            FD_FILESIZE | FD_PROGRESSUI
        } else {
            FD_PROGRESSUI
        };
        fd.nFileSizeLow = (e.size & 0xFFFF_FFFF) as u32;
        fd.nFileSizeHigh = (e.size >> 32) as u32;
        fd.cFileName = cfn;
        ptr::write_unaligned(fd_base.add(i), fd);
    }
    let _ = GlobalUnlock(hg);
    Ok(hg)
}

fn stg_hglobal(hg: HGLOBAL) -> STGMEDIUM {
    STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 { hGlobal: hg },
        pUnkForRelease: ManuallyDrop::new(None),
    }
}
fn stg_istream(s: IStream) -> STGMEDIUM {
    STGMEDIUM {
        tymed: TYMED_ISTREAM.0 as u32,
        u: STGMEDIUM_0 {
            pstm: ManuallyDrop::new(Some(s)),
        },
        pUnkForRelease: ManuallyDrop::new(None),
    }
}

// ---------------- IDataObject ----------------

#[implement(IDataObject)]
struct PromisedData {
    items: Vec<Item>,
    transport: Arc<dyn Transport>,
    rt: tokio::runtime::Handle,
    cf_descriptor: u16,
    cf_contents: u16,
    entries: OnceLock<Vec<Entry>>,
    contents: OnceLock<Vec<Vec<u8>>>,
}

impl PromisedData {
    fn get_entries(&self) -> &Vec<Entry> {
        self.entries
            .get_or_init(|| self.rt.block_on(flatten(&self.transport, &self.items)))
    }
    fn get_contents(&self) -> &Vec<Vec<u8>> {
        self.contents.get_or_init(|| {
            let paths: Vec<String> = self.get_entries().iter().map(|e| e.path.clone()).collect();
            let t = self.transport.clone();
            self.rt.block_on(async move {
                let mut handles = Vec::with_capacity(paths.len());
                for p in paths {
                    let t2 = t.clone();
                    handles.push(tokio::spawn(
                        async move { read_all(&t2, &p).await.unwrap_or_default() },
                    ));
                }
                let mut out = Vec::with_capacity(handles.len());
                for h in handles {
                    out.push(h.await.unwrap_or_default());
                }
                out
            })
        })
    }
}

impl IDataObject_Impl for PromisedData_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let fe = unsafe { &*pformatetcin };
        if fe.cfFormat == self.cf_descriptor && (fe.tymed & TYMED_HGLOBAL.0 as u32) != 0 {
            let hg = unsafe { build_descriptor(self.get_entries())? };
            return Ok(stg_hglobal(hg));
        }
        if fe.cfFormat == self.cf_contents && (fe.tymed & TYMED_ISTREAM.0 as u32) != 0 {
            let idx = fe.lindex;
            let contents = self.get_contents();
            if idx < 0 || idx as usize >= contents.len() {
                return Err(werr(DV_E_FORMATETC));
            }
            let stream = unsafe { SHCreateMemStream(Some(&contents[idx as usize])) }
                .ok_or_else(|| werr(DV_E_FORMATETC))?;
            return Ok(stg_istream(stream));
        }
        Err(werr(DV_E_FORMATETC))
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(werr(E_NOTIMPL))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        let fe = unsafe { &*pformatetc };
        let desc = fe.cfFormat == self.cf_descriptor && (fe.tymed & TYMED_HGLOBAL.0 as u32) != 0;
        let cont = fe.cfFormat == self.cf_contents && (fe.tymed & TYMED_ISTREAM.0 as u32) != 0;
        if desc || cont {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        if !pformatetcout.is_null() {
            unsafe {
                (*pformatetcout).ptd = ptr::null_mut();
            }
        }
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(werr(E_NOTIMPL));
        }
        let formats = vec![
            FORMATETC {
                cfFormat: self.cf_descriptor,
                ptd: ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0 as u32,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            },
            FORMATETC {
                cfFormat: self.cf_contents,
                ptd: ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0 as u32,
                lindex: -1,
                tymed: TYMED_ISTREAM.0 as u32,
            },
        ];
        let e: IEnumFORMATETC = FormatEnum {
            formats,
            idx: Mutex::new(0),
        }
        .into();
        Ok(e)
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: windows::core::Ref<'_, IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(werr(OLE_E_ADVISENOTSUPPORTED))
    }
    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(werr(OLE_E_ADVISENOTSUPPORTED))
    }
    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(werr(OLE_E_ADVISENOTSUPPORTED))
    }
}

// ---------------- IEnumFORMATETC ----------------

#[implement(IEnumFORMATETC)]
struct FormatEnum {
    formats: Vec<FORMATETC>,
    idx: Mutex<usize>,
}

fn copy_fe(f: &FORMATETC) -> FORMATETC {
    FORMATETC {
        cfFormat: f.cfFormat,
        ptd: ptr::null_mut(),
        dwAspect: f.dwAspect,
        lindex: f.lindex,
        tymed: f.tymed,
    }
}

impl IEnumFORMATETC_Impl for FormatEnum_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        let mut idx = self.idx.lock().unwrap();
        let mut fetched = 0u32;
        while fetched < celt && *idx < self.formats.len() {
            unsafe {
                *rgelt.add(fetched as usize) = copy_fe(&self.formats[*idx]);
            }
            *idx += 1;
            fetched += 1;
        }
        if !pceltfetched.is_null() {
            unsafe {
                *pceltfetched = fetched;
            }
        }
        if fetched == celt {
            S_OK
        } else {
            S_FALSE
        }
    }
    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        *self.idx.lock().unwrap() += celt as usize;
        Ok(())
    }
    fn Reset(&self) -> windows::core::Result<()> {
        *self.idx.lock().unwrap() = 0;
        Ok(())
    }
    fn Clone(&self) -> windows::core::Result<IEnumFORMATETC> {
        let formats = self.formats.iter().map(copy_fe).collect();
        let e: IEnumFORMATETC = FormatEnum {
            formats,
            idx: Mutex::new(*self.idx.lock().unwrap()),
        }
        .into();
        Ok(e)
    }
}

// ---------------- IDropSource ----------------

#[implement(IDropSource)]
struct DropSrc;

impl IDropSource_Impl for DropSrc_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if grfkeystate.0 & MK_LBUTTON == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }
    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Run the OLE drag on the current (main) thread. Blocks until the drop finishes.
pub fn do_drag(items: Vec<Item>, transport: Arc<dyn Transport>, rt: tokio::runtime::Handle) {
    unsafe {
        let _ = OleInitialize(None);
        let cf_descriptor = RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16;
        let cf_contents = RegisterClipboardFormatW(w!("FileContents")) as u16;
        let data: IDataObject = PromisedData {
            items,
            transport,
            rt,
            cf_descriptor,
            cf_contents,
            entries: OnceLock::new(),
            contents: OnceLock::new(),
        }
        .into();
        let src: IDropSource = DropSrc.into();
        let mut effect = DROPEFFECT::default();
        let _ = DoDragDrop(&data, &src, DROPEFFECT_COPY, &mut effect);
    }
}
