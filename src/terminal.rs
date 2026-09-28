use anyhow::{Context, ensure};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use std::{
    collections::HashSet,
    io::{self, IsTerminal, Write},
    panic::{self, PanicHookInfo},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type Hook = Arc<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;
struct Cleanup {
    done: AtomicBool,
    raw: AtomicBool,
    alternate: AtomicBool,
    ids: Mutex<HashSet<u32>>,
}
pub struct Session {
    picker: Option<Picker>,
    cleanup: Arc<Cleanup>,
    previous: Option<Hook>,
    next_id: u32,
}
fn restore(c: &Cleanup) -> anyhow::Result<()> {
    if c.done.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let ids: Vec<_> = c
        .ids
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain()
        .collect();
    let mut out = io::stdout();
    let deleted = out.write_all(&delete_commands(&ids));
    let screen = if c.alternate.swap(false, Ordering::SeqCst) {
        execute!(out, LeaveAlternateScreen, Show)
    } else {
        Ok(())
    };
    let raw = if c.raw.swap(false, Ordering::SeqCst) {
        terminal::disable_raw_mode()
    } else {
        Ok(())
    };
    deleted.and(screen).and(raw).context("端末を復元できません")
}
impl Session {
    pub fn enter() -> anyhow::Result<Self> {
        ensure!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "TTYで実行してください（stdin・stdoutが端末である必要があります）"
        );
        // Picker enables tmux passthrough as a side effect. Avoid modifying it in this version.
        ensure!(
            !std::env::var("TERM").is_ok_and(|t| t.starts_with("tmux") || t.starts_with("screen"))
                && !std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "tmux"),
            "初期版はtmux外のKitty対応端末で実行してください"
        );
        let cleanup = Arc::new(Cleanup {
            done: AtomicBool::new(false),
            raw: AtomicBool::new(false),
            alternate: AtomicBool::new(false),
            ids: Mutex::new(HashSet::new()),
        });
        let previous: Hook = panic::take_hook().into();
        let hook_cleanup = cleanup.clone();
        let hook_previous = previous.clone();
        panic::set_hook(Box::new(move |info| {
            let _ = restore(&hook_cleanup);
            hook_previous(info);
        }));
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
            ^ std::process::id();
        let mut session = Self {
            picker: None,
            cleanup,
            previous: Some(previous),
            next_id: seed % 0xff_ffff + 1,
        };
        terminal::enable_raw_mode()?;
        session.cleanup.raw.store(true, Ordering::SeqCst);
        session.cleanup.alternate.store(true, Ordering::SeqCst);
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        // The library resets its timeout after every byte; impose a whole-query deadline as well.
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = tx.send(Picker::from_query_stdio());
        });
        let picker = rx
            .recv_timeout(Duration::from_secs(3))
            .context("Kitty端末の問い合わせがタイムアウトしました")??;
        validate_picker(&picker)?;
        session.picker = Some(picker);
        Ok(session)
    }
    pub fn picker(&self) -> &Picker {
        self.picker
            .as_ref()
            .expect("initialized session has a picker")
    }
    pub fn allocate_ids(&mut self, count: usize) -> Vec<u32> {
        let mut own = self.cleanup.ids.lock().unwrap_or_else(|e| e.into_inner());
        let mut result = Vec::with_capacity(count);
        while result.len() < count {
            let id = self.next_id;
            self.next_id = if id == 0xff_ffff { 1 } else { id + 1 };
            if own.insert(id) {
                result.push(id);
            }
        }
        result
    }
    pub fn delete_ids(&mut self, ids: &[u32]) -> anyhow::Result<()> {
        let owned: Vec<_> = {
            let own = self.cleanup.ids.lock().unwrap_or_else(|e| e.into_inner());
            ids.iter().copied().filter(|id| own.contains(id)).collect()
        };
        let mut out = io::stdout();
        out.write_all(&delete_commands(&owned))?;
        out.flush()?;
        let mut own = self.cleanup.ids.lock().unwrap_or_else(|e| e.into_inner());
        for id in owned {
            own.remove(&id);
        }
        Ok(())
    }
    pub fn finish(&mut self) -> anyhow::Result<()> {
        let result = restore(&self.cleanup);
        if !std::thread::panicking()
            && let Some(previous) = self.previous.take()
        {
            panic::set_hook(Box::new(move |info| previous(info)));
        }
        result
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
pub fn validate_picker(p: &Picker) -> anyhow::Result<()> {
    ensure!(
        p.protocol_type() == ProtocolType::Kitty && p.capabilities().contains(&Capability::Kitty),
        "Kitty Graphics Protocolの対応を確認できません。Kitty対応端末で実行してください"
    );
    let size = p.font_size();
    ensure!(
        size.width > 0 && size.height > 0,
        "端末の文字セル寸法が不正です"
    );
    Ok(())
}
pub fn delete_commands(ids: &[u32]) -> Vec<u8> {
    let mut commands = String::new();
    for id in ids {
        use std::fmt::Write;
        let _ = write!(commands, "\x1b_Ga=d,d=I,i={id},q=2;\x1b\\");
    }
    commands.into_bytes()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_kitty_is_accepted() {
        assert!(validate_picker(&Picker::halfblocks()).is_err());
    }
    #[test]
    fn deletion_targets_owned_ids() {
        let bytes = delete_commands(&[100, 200]);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("a=d,d=I,i=100"));
        assert!(text.contains("a=d,d=I,i=200"));
        assert!(!text.contains("d=A"));
    }
}
