# Task checkpoints and recovery / 任务检查点与恢复

PostgreSQL checkpoints each task's goal, context, pending calls, action IDs, model revision, retry counters, step, control epoch and wake deadline. When the desktop app reopens, interrupted tasks resume from their last checkpoint after device setup confirms Android and Linux are ready. Pause and Resume keep the same task and its original wait deadline. Stop ends the task.

After a runtime restart, recovery marks an unconfirmed action as unknown, withdraws the rest of that tool batch and observes the affected phone or computer before continuing. When an action loses its receipt during a live session, the task pauses for verification. Resuming or sending a follow-up to the paused task uses the same recovery path.

## Model configuration on recovery

Model changes take effect for an existing task only when it is paused and the user explicitly resumes it or submits a new instruction to that paused user task. Recovery resolves the current model connection through the task’s original Agent identity. The existing task keeps its Agent instructions and device authorization. Finish or stop the task to switch profiles. Running and queued tasks retain their pinned model revision.

Recovery locks the conversation, then the paused run, then the configuration registry. It rechecks the conversation and Agent identity before selecting the saved revision. A replacement revision must have passed the configuration check. Selecting the revision, preserving notice provenance, resetting retry state and queueing the task commit together. A missing or unverified replacement leaves the checkpoint unchanged; a failed follow-up submission also rolls back its message and receipt.

Recovery preserves the raw context, pending calls, action IDs and execution records. Changing provider, protocol, endpoint or model establishes a trusted checkpoint boundary: outgoing requests exclude older provider-private reasoning and signatures while retaining tool results as historical evidence. New complete native tool exchanges remain available to the selected model. Credential-only recovery retains the established replay boundary. Unknown side effects still require outcome verification and are never blindly replayed. Operational failures retain their classification after recovery instead of reappearing as assistant dialogue.

## 恢复时的模型配置

已有任务只有在暂停后由用户明确恢复，或向该暂停的用户任务追加指令时，才会采用更新后的模型配置。恢复流程通过任务原有的 Agent 身份读取当前模型连接，保留原任务的 Agent 指令与设备授权。完成或停止任务后可切换配置。正在运行或排队的任务继续使用已固定的模型版本。

恢复依次锁定对话、暂停任务与配置注册表，重新核对对话及 Agent 身份后选择已保存版本。替换版本必须通过配置检查。模型选择、通知来源保留、重试状态重置与重新排队在同一事务提交。替换版本缺失或未验证时，检查点保持不变；追加指令失败时，对应消息与接收凭据也整体回滚。

恢复过程保留原始上下文、待执行工具、动作 ID 和执行记录。更换服务商、协议、端点或模型后，出站请求按可信检查点边界移除旧模型的私有推理和签名，同时将原工具结果保留为历史证据；新模型的完整原生工具往返仍可正常传递。仅修正密钥时保留已有边界。结果未知的副作用仍须核对，不直接重放。运行故障在恢复后仍归类为系统通知，不重新进入助手对话。

## Failure behavior

| Failure | Behavior |
| --- | --- |
| Worker killed/restarted | Reload the checkpoint, reacquire phone control as needed and reuse completed tool results |
| Persisted wait | Retain its original deadline; one wake event when due |
| PostgreSQL unavailable after claim | Hold the claimed run; retry only its idempotent result transaction after recovery |
| In-flight side effect has no result | Recover from a fresh observation after restart; pause for verification during a live session |
| Provider 503 / temporary 429 | Persist bounded backoff/retry count, including across restart |
| Explicit exhausted quota | Pause immediately; preserve task and records |
| Partial/incomplete model stream | Discard unfinished output; no speculative tool execution |
| Long foreground task | Continue in slices; preserve completed effects |

Runtime result finalization locks the run and updates the run, message and top-level event in one transaction. It preserves cancellation and pending user steering. If PostgreSQL is unavailable, the worker holds the claimed run and retries finalization. Unresolved tool records pause the run.

Host sleep/reboot, power-loss recovery and prolonged multi-device operation have separate platform qualification. Current availability is maintained in [release status](release-status.md).

## 任务中断与继续

重新打开桌面应用时，Android 和 Linux 通过就绪检查后，退出前运行中的任务从最近的检查点继续。对话底部的「暂停」保留同一任务和原等待截止时间，「继续」恢复执行，「停止」结束任务。

服务重启后，执行器保留结果未知的记录，撤回同批尚未执行的操作，重新读取受影响的设备后续跑。运行中丢失操作回执会暂停，点击继续或追加指令后走同一恢复路径。

## 安装中断

安装阶段依次展开离线资源、启动 Android 和 Linux、注册输入与服务，再从两台设备读取状态。重开客户端会复用已完成的资源，继续未完成的阶段。安装和恢复使用同一个本地配置与设备盘。

源码部署可再次运行 `./ai-phone install --resources /path/to/host`。`stop` 会停止当前安装管理的进程并保留持久状态。
