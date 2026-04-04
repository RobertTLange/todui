#[cfg(not(test))]
use crate::error::Result;

#[cfg(not(test))]
pub(crate) fn open_url(url: &str) -> Result<()> {
    use std::io::Error;
    use std::process::Command;

    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status()?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(url).status()?;

    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(Error::other(format!("browser opener exited with status {status}")).into())
    }
}

#[cfg(test)]
pub(crate) use test_browser::{
    open_url, reset as reset_test_browser, set_should_fail as set_test_browser_should_fail,
    take_opened_urls as take_test_browser_opened_urls,
};

#[cfg(test)]
mod test_browser {
    use std::cell::RefCell;
    use std::io::Error;

    use crate::error::Result;

    #[derive(Debug, Default)]
    struct TestBrowserState {
        opened_urls: Vec<String>,
        should_fail: bool,
    }

    thread_local! {
        static TEST_BROWSER_STATE: RefCell<TestBrowserState> = RefCell::new(TestBrowserState::default());
    }

    pub(crate) fn open_url(url: &str) -> Result<()> {
        TEST_BROWSER_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.opened_urls.push(url.to_string());
            if state.should_fail {
                Err(Error::other("test browser open failure").into())
            } else {
                Ok(())
            }
        })
    }

    pub(crate) fn reset() {
        TEST_BROWSER_STATE.with(|state| {
            *state.borrow_mut() = TestBrowserState::default();
        });
    }

    pub(crate) fn take_opened_urls() -> Vec<String> {
        TEST_BROWSER_STATE.with(|state| std::mem::take(&mut state.borrow_mut().opened_urls))
    }

    pub(crate) fn set_should_fail(should_fail: bool) {
        TEST_BROWSER_STATE.with(|state| {
            state.borrow_mut().should_fail = should_fail;
        });
    }
}
