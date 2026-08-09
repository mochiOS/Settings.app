use std::fs;
use std::io;
use std::path::Path;

use mochios_user_database::{FIRST_REGULAR_UID, UserDatabase, UserRecord};

use mochios_user_protocol::{
    AddUser, MAX_CHUNK_LEN, MAX_MESSAGE_LEN, RemoveUser, SetPassword, SnapshotChunk,
    SnapshotChunkRequest, SnapshotInfo, SnapshotRequest, Status,
};

const HOME_DIRECTORIES: [&str; 6] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
];

pub(crate) fn load() -> io::Result<UserDatabase> {
    #[cfg(target_os = "mochios")]
    {
        return load_from_service();
    }
    #[cfg(not(target_os = "mochios"))]
    Ok(UserDatabase::with_root())
}

pub(crate) fn add(name: &str, display_name: &str, password: &[u8]) -> io::Result<()> {
    let database = load()?;
    let uid = database
        .next_regular_uid()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .max(FIRST_REGULAR_UID);
    let mut user = UserRecord::regular(name, uid, uid);
    user.display_name = display_name.trim().to_owned();
    user.validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let encoded = user
        .encode()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    create_home(&user)?;
    if let Err(error) = mutate(|request_id, output| {
        AddUser {
            request_id,
            encoded_record: &encoded,
        }
        .encode(output)
    }) {
        rollback_home(&user.home);
        return Err(error);
    }
    if let Err(error) = set_password(&user.name, password) {
        if remove(&user.name).is_ok() {
            rollback_home(&user.home);
        }
        return Err(error);
    }
    Ok(())
}

fn rollback_home(home: &str) {
    let home = Path::new(home);
    for directory in HOME_DIRECTORIES.iter().rev() {
        let _ = fs::remove_dir(home.join(directory));
    }
    let _ = fs::remove_dir(home);
}

pub(crate) fn remove(name: &str) -> io::Result<()> {
    mutate(|request_id, output| RemoveUser { request_id, name }.encode(output))
}

pub(crate) fn set_password(name: &str, password: &[u8]) -> io::Result<()> {
    let result = mutate(|request_id, output| {
        SetPassword {
            request_id,
            name,
            password,
        }
        .encode(output)
    });
    result
}

fn create_home(user: &UserRecord) -> io::Result<()> {
    let home = Path::new(&user.home);
    if home != Path::new("/home").join(&user.name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid managed home path",
        ));
    }
    fs::create_dir(home)?;
    let initialized = (|| {
        for directory in HOME_DIRECTORIES {
            fs::create_dir(home.join(directory))?;
        }
        #[cfg(target_os = "mochios")]
        {
            use std::os::unix::fs::{PermissionsExt, chown};
            fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
            chown(home, Some(user.uid), Some(user.gid))?;
            for directory in HOME_DIRECTORIES {
                let path = home.join(directory);
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                chown(&path, Some(user.uid), Some(user.gid))?;
            }
        }
        Ok(())
    })();
    if initialized.is_err() {
        rollback_home(&user.home);
    }
    initialized
}

#[cfg(target_os = "mochios")]
fn load_from_service() -> io::Result<UserDatabase> {
    const MAX_DATABASE_BYTES: usize = 1024 * 1024;
    for _ in 0..3 {
        let service = find_service()?;
        let request_id = next_request_id();
        let mut request = [0u8; mochios_user_protocol::SNAPSHOT_REQUEST_LEN];
        let length = SnapshotRequest { request_id }
            .encode(&mut request)
            .map_err(protocol_encode)?;
        let mut reply = [0u8; MAX_MESSAGE_LEN];
        let reply_len = call(service, &request[..length], &mut reply)?;
        if let Ok(status) = Status::decode(&reply[..reply_len]) {
            return Err(status_error(status));
        }
        let info = SnapshotInfo::decode(&reply[..reply_len]).map_err(protocol_decode)?;
        if info.request_id != request_id {
            return Err(invalid_response("snapshot request ID mismatch"));
        }
        let total = usize::try_from(info.total_len)
            .ok()
            .filter(|length| *length > 0 && *length <= MAX_DATABASE_BYTES)
            .ok_or_else(|| invalid_response("invalid snapshot length"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total)
            .map_err(|_| io::Error::from_raw_os_error(libc::ENOMEM))?;
        while bytes.len() < total {
            let wanted = (total - bytes.len()).min(MAX_CHUNK_LEN);
            let chunk_request = SnapshotChunkRequest {
                request_id,
                offset: bytes.len() as u64,
                length: wanted as u32,
            };
            let mut request = [0u8; mochios_user_protocol::CHUNK_REQUEST_LEN];
            let length = chunk_request
                .encode(&mut request)
                .map_err(protocol_encode)?;
            let reply_len = call(service, &request[..length], &mut reply)?;
            let chunk = SnapshotChunk::decode(&reply[..reply_len]).map_err(protocol_decode)?;
            if chunk.request_id != request_id
                || chunk.offset != bytes.len() as u64
                || chunk.generation != info.generation
                || chunk.bytes.len() > total - bytes.len()
            {
                bytes.clear();
                break;
            }
            bytes.extend_from_slice(chunk.bytes);
        }
        if bytes.len() == total {
            return UserDatabase::parse(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
    }
    Err(invalid_response("user database changed during snapshot"))
}

#[cfg(target_os = "mochios")]
fn mutate(
    encode: impl FnOnce(u64, &mut [u8]) -> Result<usize, mochios_user_protocol::EncodeError>,
) -> io::Result<()> {
    let service = find_service()?;
    let request_id = next_request_id();
    let mut request = [0u8; MAX_MESSAGE_LEN];
    let length = encode(request_id, &mut request).map_err(protocol_encode)?;
    let mut reply = [0u8; mochios_user_protocol::STATUS_LEN];
    let reply_len = call(service, &request[..length], &mut reply)?;
    request[..length].fill(0);
    let status = Status::decode(&reply[..reply_len]).map_err(protocol_decode)?;
    if status.request_id != request_id {
        return Err(invalid_response("mutation request ID mismatch"));
    }
    if status.status == 0 {
        Ok(())
    } else {
        Err(status_error(status))
    }
}

#[cfg(not(target_os = "mochios"))]
fn mutate(
    _encode: impl FnOnce(u64, &mut [u8]) -> Result<usize, mochios_user_protocol::EncodeError>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "user.service is available only on mochiOS",
    ))
}

#[cfg(target_os = "mochios")]
fn find_service() -> io::Result<u64> {
    for _ in 0..64 {
        if let Ok(endpoint) = mochi_user_platform::process::find_by_name("user.service")
            && endpoint != 0
        {
            return Ok(endpoint);
        }
        mochi_user_platform::thread::yield_now();
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "user.service was not found",
    ))
}

#[cfg(target_os = "mochios")]
fn call(destination: u64, request: &[u8], reply: &mut [u8]) -> io::Result<usize> {
    let result = mochi_user_platform::ipc::call(destination, request, reply).map_err(|error| {
        io::Error::from_raw_os_error(error.errno().unwrap_or(libc::EIO as u64) as i32)
    })?;
    let length = (result & 0xffff_ffff) as usize;
    if length > reply.len() {
        return Err(invalid_response("invalid IPC reply length"));
    }
    Ok(length)
}

#[cfg(target_os = "mochios")]
fn next_request_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed).max(1)
}

#[cfg(target_os = "mochios")]
fn status_error(status: Status) -> io::Error {
    io::Error::from_raw_os_error(status.status.checked_neg().unwrap_or(libc::EIO))
}

fn protocol_encode(error: mochios_user_protocol::EncodeError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("{error:?}"))
}

fn protocol_decode(error: mochios_user_protocol::DecodeError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}"))
}

fn invalid_response(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
