//! Filter PTY bytes before they enter the frontend mailbox or terminal state.

use devenv_mailbox::ShellOutputFilter;
use std::io;

/// Read until filtered output is available or the PTY ends. `false` means EOF;
/// output can still contain the final suffix and must be delivered first.
pub(crate) fn read_filtered_output(
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
    filter: &mut Option<Box<dyn ShellOutputFilter>>,
    input: &mut [u8],
    output: &mut Vec<u8>,
) -> io::Result<bool> {
    output.clear();
    loop {
        let count = match read(input) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // Unix PTYs can report EIO when the slave closes instead of EOF.
            #[cfg(unix)]
            Err(error) if error.raw_os_error() == Some(libc::EIO) => 0,
            Err(error) => return Err(error),
        };
        if count == 0 {
            if let Some(filter) = filter.take() {
                filter.finish(output);
            }
            return Ok(false);
        }
        if let Some(filter) = filter {
            filter.push(&input[..count], output);
        } else {
            output.extend_from_slice(&input[..count]);
        }
        input[..count].fill(0);
        if !output.is_empty() {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    // Retaining all input makes it easy to detect accidental raw forwarding,
    // premature EOF, or losing the final buffered output.
    #[derive(Debug, Default)]
    struct UntilEof(Vec<u8>);

    impl ShellOutputFilter for UntilEof {
        fn push(&mut self, input: &[u8], _: &mut Vec<u8>) {
            self.0.extend_from_slice(input);
        }

        fn finish(self: Box<Self>, output: &mut Vec<u8>) {
            assert_eq!(self.0, b"private");
            output.extend_from_slice(b"[FILTERED]");
        }
    }

    #[test]
    fn buffered_reads_are_not_forwarded_and_eof_emits_filtered_suffix() {
        let mut source = &b"private"[..];
        let mut filter: Option<Box<dyn ShellOutputFilter>> = Some(Box::<UntilEof>::default());
        let mut input = [0; 2];
        let mut output = Vec::new();
        assert!(
            !read_filtered_output(|buf| source.read(buf), &mut filter, &mut input, &mut output,)
                .unwrap()
        );
        assert_eq!(output, b"[FILTERED]");
        assert!(filter.is_none());
    }

    #[test]
    fn read_failure_does_not_flush_pending_bytes() {
        let mut filter: Option<Box<dyn ShellOutputFilter>> = Some(Box::<UntilEof>::default());
        let mut first = true;
        let mut output = Vec::new();
        let error = read_filtered_output(
            |buf| {
                if first {
                    first = false;
                    buf[..7].copy_from_slice(b"private");
                    Ok(7)
                } else {
                    Err(io::Error::other("read failure"))
                }
            },
            &mut filter,
            &mut [0; 8],
            &mut output,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(output.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn pty_eio_finishes_the_filter_like_eof() {
        let mut filter: Option<Box<dyn ShellOutputFilter>> =
            Some(Box::new(UntilEof(b"private".to_vec())));
        let mut output = Vec::new();
        assert!(
            !read_filtered_output(
                |_| Err(io::Error::from_raw_os_error(libc::EIO)),
                &mut filter,
                &mut [0; 8],
                &mut output,
            )
            .unwrap()
        );
        assert_eq!(output, b"[FILTERED]");
        assert!(filter.is_none());
    }
}
