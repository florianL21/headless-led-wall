use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use ekv::flash::{self, PageID};
use ekv::{config, Database, ReadError};
use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use esp_backtrace as _;
use esp_bootloader_esp_idf::partitions::{self, FlashRegion};
use esp_hal::system::{Cpu, CpuControl};
use esp_storage::FlashStorage;
use log::info;
use static_cell::make_static;

pub type FlashType =
    Database<PersistentStorage<FlashRegion<'static, FlashStorage>>, CriticalSectionRawMutex>;

pub enum FlashOperation {
    Store(String, Vec<u8>),
    Delete(String),
    Exists(String),
    Format,
}

pub type FlashOperationChannel = Channel<CriticalSectionRawMutex, FlashOperation, 3>;
pub static FLASH_OPERATION: FlashOperationChannel = Channel::new();

#[derive(Debug)]
pub enum FlashOperationResult {
    WriteErr(ekv::WriteError<partitions::Error>),
    CommitErr(ekv::CommitError<partitions::Error>),
    FormatErr(ekv::FormatError<partitions::Error>),
    ReadErr(ekv::ReadError<partitions::Error>),
    Error(ekv::Error<partitions::Error>),
    // Ugly hack because I'm too lazy to make a proper type for this now
    ExistsResult(bool),
}

pub type FlashOperationResultSignal =
    Signal<CriticalSectionRawMutex, Result<(), FlashOperationResult>>;
pub static FLASH_OPERATION_RESULT: FlashOperationResultSignal = Signal::new();

/// Make a zeroed out buffer in heap
pub fn make_buf() -> Box<[u8]> {
    let buf = Box::new_zeroed_slice(10240);
    unsafe { buf.assume_init() }
}

// Workaround for alignment requirements.
#[repr(C, align(4))]
struct AlignedBuf<const N: usize>([u8; N]);

pub struct PersistentStorage<T: NorFlash + ReadNorFlash> {
    start: usize,
    flash: T,
}

impl<T: NorFlash + ReadNorFlash> flash::Flash for PersistentStorage<T> {
    type Error = T::Error;

    fn page_count(&self) -> usize {
        config::MAX_PAGE_COUNT
    }

    async fn erase(
        &mut self,
        page_id: PageID,
    ) -> Result<(), <PersistentStorage<T> as flash::Flash>::Error> {
        let from = (self.start + page_id.index() * config::PAGE_SIZE) as u32;
        let to = (self.start + page_id.index() * config::PAGE_SIZE + config::PAGE_SIZE) as u32;
        self.flash.erase(from, to)
    }

    async fn read(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), <PersistentStorage<T> as flash::Flash>::Error> {
        let address = self.start + page_id.index() * config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; config::PAGE_SIZE]);
        self.flash.read(address as u32, &mut buf.0[..data.len()])?;
        data.copy_from_slice(&buf.0[..data.len()]);
        Ok(())
    }

    async fn write(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), <PersistentStorage<T> as flash::Flash>::Error> {
        let address = self.start + page_id.index() * config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; config::PAGE_SIZE]);
        buf.0[..data.len()].copy_from_slice(data);
        self.flash.write(address as u32, &buf.0[..data.len()])
    }
}

pub fn flash_init() -> FlashType {
    let flash = make_static!(FlashStorage::new());
    let pt_mem = make_static!([0u8; partitions::PARTITION_TABLE_MAX_LEN]);
    let pt = partitions::read_partition_table(flash, pt_mem).unwrap();
    let fat = make_static!(pt
        .find_partition(partitions::PartitionType::Data(
            partitions::DataPartitionSubType::LittleFs,
        ))
        .expect("Failed to search for partitions")
        .expect("Could not find a data:littlefs partition"));
    let offset = fat.offset();
    info!("Storing data into partition with offset: {offset}");
    let fat_partition = fat.as_embedded_storage(flash);

    let flash = PersistentStorage {
        flash: fat_partition,
        start: 0,
    };

    Database::<_, CriticalSectionRawMutex>::new(flash, ekv::Config::default())
}

#[task]
pub async fn flash_task(flash: &'static FlashType, mut cpu_control: CpuControl<'static>) {
    if flash.mount().await.is_err() {
        info!("Flash mount failed. Formatting...");
        unsafe {
            cpu_control.park_core(Cpu::AppCpu);
        }
        flash.format().await.unwrap();
        cpu_control.unpark_core(Cpu::AppCpu);
    }
    info!("Flash task is starting");
    loop {
        let operation = FLASH_OPERATION.receive().await;
        match operation {
            FlashOperation::Format => {
                info!("Formatting flash...");
                unsafe {
                    cpu_control.park_core(Cpu::AppCpu);
                }
                FLASH_OPERATION_RESULT.signal(
                    flash
                        .format()
                        .await
                        .map_err(FlashOperationResult::FormatErr),
                );
                cpu_control.unpark_core(Cpu::AppCpu);
            }
            FlashOperation::Delete(ref key) => {
                info!("Deleting {key}...");
                unsafe {
                    cpu_control.park_core(Cpu::AppCpu);
                }
                let mut wtx = flash.write_transaction().await;
                if let Err(e) = wtx.delete(key.as_bytes()).await {
                    FLASH_OPERATION_RESULT.signal(Err(FlashOperationResult::WriteErr(e)));
                } else {
                    FLASH_OPERATION_RESULT
                        .signal(wtx.commit().await.map_err(FlashOperationResult::CommitErr));
                }
                cpu_control.unpark_core(Cpu::AppCpu);
            }
            FlashOperation::Store(ref key, ref value) => {
                info!("Saving {key} to flash...");
                unsafe {
                    cpu_control.park_core(Cpu::AppCpu);
                }
                let mut wtx = flash.write_transaction().await;
                if let Err(e) = wtx.write(key.as_bytes(), value.as_slice()).await {
                    FLASH_OPERATION_RESULT.signal(Err(FlashOperationResult::WriteErr(e)));
                } else {
                    FLASH_OPERATION_RESULT
                        .signal(wtx.commit().await.map_err(FlashOperationResult::CommitErr));
                    info!("Done");
                }
                cpu_control.unpark_core(Cpu::AppCpu);
            }
            FlashOperation::Exists(ref key) => {
                info!("Checking if {key} exists...");
                let wtx = flash.read_transaction().await;
                let mut val_buf = make_buf();
                match wtx.read(key.as_bytes(), &mut val_buf).await {
                    Ok(_) => {
                        FLASH_OPERATION_RESULT.signal(Err(FlashOperationResult::ExistsResult(true)))
                    }
                    Err(e) => match e {
                        ReadError::KeyNotFound => FLASH_OPERATION_RESULT
                            .signal(Err(FlashOperationResult::ExistsResult(false))),
                        e => FLASH_OPERATION_RESULT.signal(Err(FlashOperationResult::ReadErr(e))),
                    },
                }
            }
        }
    }
}
