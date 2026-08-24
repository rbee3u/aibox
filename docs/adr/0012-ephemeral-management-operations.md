# Keep long-running Management Operations ephemeral and singular

One aibox Service allows one active long-running Management Operation: Docker
image construction or a Component installation. The latest Operation is held
only in Service memory with cancellation state, monotonic log sequence numbers,
and a one MiB retained log window. A second Operation receives `Busy`; Service
shutdown requests cancellation and waits for cleanup.

This mechanism is not Operation History, Run History, or cross-process
coordination. Ordinary management mutations remain short serialized writes,
and `aibox run` continues to coordinate only its own container lifecycle.
