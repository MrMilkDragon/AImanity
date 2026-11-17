# Workspace execution

`workspace_run` starts a Rust-supervised shell job in the Linux guest. The job mounts the shared system read-only. Its `/workspace` files appear in the shared file manager and `/skills` contains the current skill resources. The graphical desktop has a separate writable layer.

The Linux process layer applies CPU, memory and process limits and opens a separate network namespace for each job. Tasks keep running when the user switches devices. The assistant drains output and retains up to 64 KiB. The guest enforces a 90-second deadline.

Cancellation closes the job's input lease. Exit code 130 confirms cancellation; 124 reports the guest deadline. The assistant reports an unconfirmed result when the guest cannot acknowledge the cancellation promptly. A job's output already saved to `/workspace` stays there.
