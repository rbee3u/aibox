# Supervise Docker container lifecycle in the wrapper

Runs and toolchain installations stay under wrapper supervision instead of
replacing the process with `docker run`; aibox tracks both the Docker child and
container identity and permits one active container operation per process.
This adds signal and cleanup coordination but prevents the Docker client and
container lifecycles from silently diverging and leaving an unobserved live
container.
