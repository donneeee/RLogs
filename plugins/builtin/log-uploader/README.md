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

Website authentication, device authorization, concrete HTTP transport, and
automatic submission are not connected yet. Those future layers must remain
host-owned rather than exposing credentials to this plug-in. Immediately
before any real transport, the host must invoke Core's full queued-artifact
verification; file-size checks and earlier UI results are not upload
authorization.
