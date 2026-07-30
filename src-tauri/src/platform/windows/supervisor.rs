//! Children die with this process, however it dies
//! ([architecture.md](../../../../docs/architecture.md) §Process/threading).

/// Put this process in a Windows job object that kills every child when the
/// process dies — *however* it dies. A clean quit already stops the sidecars
/// (`RunEvent::Exit`), but a dev-loop rebuild, a crash, or a task-manager
/// kill skips that path and used to orphan `llama-server.exe`, which then
/// held its own files open and made every re-download fail with "access
/// denied" (the NTFS delete-pending trap).
pub fn kill_children_with_us() {
    use windows::core::PCWSTR;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let Ok(job) = CreateJobObjectW(None, PCWSTR::null()) else {
            tracing::warn!("failed to create the child-process job object");
            return;
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set.is_err() || AssignProcessToJobObject(job, GetCurrentProcess()).is_err() {
            tracing::warn!("failed to arm the child-process job object");
        }
        // The job handle is deliberately never closed: it closes when this
        // process exits, and that close is what takes the children down.
    }
}

/// Children inherit the job object automatically — nothing per-spawn.
pub fn prepare_spawn(_cmd: &mut std::process::Command) {}

/// [`prepare_spawn`] for tokio-spawned children — nothing per-spawn.
pub fn prepare_spawn_tokio(_cmd: &mut tokio::process::Command) {}

/// The job object tracks children by itself — nothing to register.
pub fn watch_child(_pid: u32) {}
