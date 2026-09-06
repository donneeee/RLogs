# Log uploader

Resumable upload of sanitized `.rlog` artifacts using revocable per-device
website authorization.

The desktop publishes this built-in as its own **Log Uploader** workspace.
It is disabled by default and stores consent, automatic-upload preference,
default visibility, and successful-receipt retention policy in an atomically
replaced host settings file. Capturing and producing verified local drafts
does not require enabling it.

The Queue tab can run the real resumable state machine against a bounded local
mock receiver. The mock re-verifies the exact sealed artifact, validates each
chunk digest, forces a serialized mid-upload restart, resumes from
acknowledgements, verifies the final receipt, makes zero external requests, and
never deletes the artifact.

Website authentication, device authorization, HTTP transport, and automatic
submission are implemented by the desktop host; credentials are not exposed
to this plug-in. Immediately before transport, the host reopens the sealed
artifact and performs full queued-artifact verification. File-size checks
and earlier UI results are not upload authorization.

## Receiver outages

An HTTP failure (including 503 or 530) leaves the persisted submission draft
pending. The host marks it submitted only after transport returns a validated
receipt. With Log Uploader enabled, automatic combat-log uploads enabled, and
an account connection configured, failures retry with exponential backoff
from 5 seconds up to 5 minutes. Pending drafts are considered in rotation so
one failed draft does not permanently starve the others.

Each new attempt sends the sealed manifest again. The receiver returns the
missing chunks for a partial upload or the existing report for an already
completed artifact. The uploader does not delete the sealed artifact on an
HTTP failure. Queue files survive an app restart; the in-memory retry delay
does not. This protects already queued artifacts, not runs that were never
successfully captured and sealed.

The gateway converts upstream 5xx/network failures to an uncached JSON 503
with a Retry-After hint. The current desktop transport uses HTTP status and
its own backoff; it does not yet consume that hint. Restoring the receiver's
public connectivity is still required before queued submissions can finish.
