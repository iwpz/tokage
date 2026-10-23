import { useTranslation } from "react-i18next";
import type { RuntimeSnapshot } from "../../lib/runtime";

/** A read-only projection of the current review; never infers acceptance from prose. */
export function RuntimeDeliveryEvidence({ snapshot }: { snapshot: RuntimeSnapshot }) {
  const { t } = useTranslation();
  const delivery = snapshot.delivery;
  if (!delivery) return null;
  const reviewer = delivery.reviewId ? snapshot.turns?.[delivery.reviewId] : undefined;
  const loading = snapshot.state === "Reviewing";
  const rows = [
    [t("runtime.model"), reviewer?.observedModel ?? t("runtime.unknownModel")],
    [t("runtime.cost", { cost: reviewer?.costUsd == null ? t("runtime.unknownValue") : `$${reviewer.costUsd.toFixed(6)}` }), null],
    [t("runtime.reviewCleanup"), t(reviewer?.cleanupConfirmed ? "runtime.cleanupConfirmed" : "runtime.cleanupPending")],
    [t("runtime.candidateCommit"), delivery.candidate?.head ?? t("runtime.unknownValue")],
    [t("runtime.originalTarget"), snapshot.config.targetIdentity],
    ...(delivery.artifact ? [[t("runtime.deliveryArtifact"), delivery.artifact]] : []),
  ];
  return <details className="mx-4 shrink-0 rounded-xl border border-border bg-elevated p-3 text-xs" aria-busy={loading}>
    <summary className="cursor-pointer font-medium text-fg">{t("runtime.deliveryEvidence")}{loading && <span role="status" className="ml-2 animate-pulse text-muted">{t("runtime.reviewLoading")}</span>}</summary>
    <dl className="mt-3 grid min-w-0 gap-2">{rows.map(([label, value]) => <div key={label} className="min-w-0"><dt className="text-muted">{label}</dt>{value != null && <dd className="break-all text-fg">{value}</dd>}</div>)}</dl>
    <p className="mt-3 text-muted">{t("runtime.reviewResult")}</p>
    {delivery.review == null ? <p>{t("runtime.reviewNotRecorded")}</p> : <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words">{JSON.stringify(delivery.review, null, 2)}</pre>}
    {delivery.error && <p role="alert" className="mt-2 break-words text-danger">{delivery.error}</p>}
  </details>;
}
