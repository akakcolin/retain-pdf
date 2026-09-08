import { TEXT_KEYS } from "../../../shared/dom/text-keys.js";
import { $ } from "../../../shared/dom/query.js";
import { firstJobIdFromPayload, firstNonEmptyText, buildDetailPageUrl } from "./routing.js";

export function summarizeResumePlan(plan) {
  if (!plan) {
    return "当前任务暂不可恢复。";
  }
  if (!plan.can_resume) {
    return plan.reason || "当前任务暂不可恢复。";
  }
  const fromStage = firstNonEmptyText(plan.from_stage, plan.resume_from, "checkpoint");
  const workflow = firstNonEmptyText(plan.resume_workflow, plan.workflow);
  const reruns = Array.isArray(plan.reruns_stages) ? plan.reruns_stages.join("、") : "";
  const bits = [`可从 ${fromStage} 恢复`];
  if (workflow) {
    bits.push(`workflow=${workflow}`);
  }
  if (reruns) {
    bits.push(`重跑 ${reruns}`);
  }
  return bits.join("，");
}

export function bindRerunButton({
  detailPageState,
  getJobId,
  resumePort,
  setText,
}) {
  $("detail-rerun-btn")?.addEventListener("click", async () => {
    const button = $("detail-rerun-btn") as any;
    const jobId = detailPageState.job?.job_id || getJobId();
    const actionUrl = `${detailPageState.rerunActionUrl || ""}`.trim();
    if (!button || (!jobId && !actionUrl)) {
      setText(TEXT_KEYS.detailRerunStatus, "当前任务暂不可从断点恢复。");
      return;
    }
    button.disabled = true;
    setText(TEXT_KEYS.detailRerunStatus, "正在提交恢复任务...");
    try {
      const payload = await resumePort.submit({ actionUrl, jobId });
      const nextJobId = firstJobIdFromPayload(payload);
      if (!nextJobId) {
        setText(TEXT_KEYS.detailRerunStatus, "恢复任务已提交，但响应中没有 job_id。");
        return;
      }
      setText(TEXT_KEYS.detailRerunStatus, `已创建恢复任务 ${nextJobId}，正在跳转...`);
      window.location.href = buildDetailPageUrl(nextJobId);
    } catch (error) {
      setText(TEXT_KEYS.detailRerunStatus, error.message || String(error));
      button.disabled = false;
    }
  });
}
