import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { ShieldCheck } from "lucide-react";
import { useSettings } from "../stores/settings";
import { Logo } from "./Logo";
import { SourceNotice } from "./SourceNotice";
import { Button } from "./ui";

/** First-launch page: what Tokage reads locally and how to check the build's origin (docs/11 §7.1). */
export function WelcomeNotice() {
  const { t } = useTranslation();
  const seen = useSettings((s) => s.welcomeSeen);
  const set = useSettings((s) => s.set);
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (!seen && !d.open) d.showModal();
    if (seen && d.open) d.close();
  }, [seen]);

  const done = () => set("welcomeSeen", true);

  return (
    <dialog
      ref={dialog}
      aria-labelledby="welcome-title"
      onCancel={done}
      className="m-auto w-[34rem] max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-elevated p-0 text-fg shadow-xl backdrop:bg-black/40"
    >
      {!seen && (
        <div className="space-y-4 p-5">
          <div className="flex items-center gap-3">
            <Logo className="size-10" />
            <div>
              <h2 id="welcome-title" className="text-[15px] font-semibold">
                {t("welcome.title")}
              </h2>
              <p className="text-xs text-muted">{t("settings.about.desc")}</p>
            </div>
          </div>
          <div className="rounded-lg border border-border bg-bg p-3.5">
            <h3 className="mb-2 flex items-center gap-1.5 text-[13px] font-medium">
              <ShieldCheck aria-hidden className="size-4 text-accent" />
              {t("welcome.boundary")}
            </h3>
            <SourceNotice variant="full" />
          </div>
          <p className="text-xs leading-relaxed text-subtle">{t("welcome.later")}</p>
          <div className="flex justify-end">
            <Button variant="primary" onClick={done} autoFocus>
              {t("welcome.ok")}
            </Button>
          </div>
        </div>
      )}
    </dialog>
  );
}
