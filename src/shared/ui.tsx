// Small UI kit for the main window (section 8). Keyboard reachable, labelled.
import { Loader2, X } from "lucide-react";
import { useEffect, useRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";

export function Button({
  variant = "secondary",
  busy,
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; busy?: boolean }) {
  const styles: Record<Variant, string> = {
    primary: "bg-[var(--accent)] text-[#0b0b10] hover:brightness-110 border-transparent font-medium",
    secondary: "bg-[var(--panel-2)] text-[var(--fg)] hover:brightness-110 border-[var(--border)]",
    ghost: "bg-transparent text-[var(--fg)] hover:bg-[var(--panel-2)] border-transparent",
    danger: "bg-transparent text-[var(--danger)] hover:bg-[color-mix(in_srgb,var(--danger)_12%,transparent)] border-[color-mix(in_srgb,var(--danger)_40%,transparent)]",
  };
  return (
    <button
      type="button"
      className={`inline-flex items-center justify-center gap-1.5 rounded-[var(--radius)] border px-3 h-8 text-[13px] transition disabled:opacity-50 disabled:pointer-events-none ${styles[variant]} ${className}`}
      disabled={busy || rest.disabled}
      {...rest}
    >
      {busy && <Loader2 size={14} className="animate-spin" aria-hidden />}
      {children}
    </button>
  );
}

export function IconButton({ label, children, className = "", ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`inline-flex items-center justify-center rounded-[var(--radius)] h-8 w-8 text-[var(--muted)] hover:text-[var(--fg)] hover:bg-[var(--panel-2)] transition disabled:opacity-40 ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}

export function Toggle({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition disabled:opacity-50 ${checked ? "bg-[var(--accent)]" : "bg-[var(--border)]"}`}
    >
      <span className={`inline-block h-4 w-4 rounded-full bg-white shadow transition ${checked ? "translate-x-[18px]" : "translate-x-0.5"}`} />
    </button>
  );
}

export function Input({ className = "", ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`h-8 rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] px-2.5 text-[13px] text-[var(--fg)] placeholder:text-[var(--muted)] focus:border-[var(--accent)] outline-none ${className}`}
      {...rest}
    />
  );
}

export function Select({ className = "", children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={`h-8 rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] px-2 text-[13px] text-[var(--fg)] focus:border-[var(--accent)] outline-none ${className}`}
      {...rest}
    >
      {children}
    </select>
  );
}

export function Row({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-2.5 border-b border-[var(--border)] last:border-b-0">
      <div className="min-w-0">
        <div className="text-[14px]">{label}</div>
        {hint && <div className="text-[12px] text-[var(--muted)] mt-0.5">{hint}</div>}
      </div>
      <div className="shrink-0 flex items-center gap-2">{children}</div>
    </div>
  );
}

export function Section({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="mb-6">
      <div className="flex items-center justify-between mb-2">
        <h2 className="text-[12px] uppercase tracking-wide font-semibold text-[var(--muted)]">{title}</h2>
        {actions}
      </div>
      <div className="rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] px-4 py-1">{children}</div>
    </section>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="py-6 text-center text-[13px] text-[var(--muted)]">{children}</div>;
}

export function Badge({ tone = "muted", children }: { tone?: "muted" | "ok" | "danger" | "warn" | "accent"; children: ReactNode }) {
  const c: Record<string, string> = {
    muted: "text-[var(--muted)] bg-[var(--panel-2)]",
    ok: "text-[var(--ok)] bg-[color-mix(in_srgb,var(--ok)_14%,transparent)]",
    danger: "text-[var(--danger)] bg-[color-mix(in_srgb,var(--danger)_14%,transparent)]",
    warn: "text-[var(--warn)] bg-[color-mix(in_srgb,var(--warn)_16%,transparent)]",
    accent: "text-[var(--accent)] bg-[color-mix(in_srgb,var(--accent)_16%,transparent)]",
  };
  return <span className={`inline-flex items-center rounded px-1.5 py-0.5 text-[11px] font-medium ${c[tone]}`}>{children}</span>;
}

export function Dialog({ title, children, onClose, actions }: { title: string; children: ReactNode; onClose: () => void; actions: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("input, select, textarea, button")?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      prev?.focus();
    };
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onMouseDown={onClose}>
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="w-[440px] max-w-[92vw] rounded-[calc(var(--radius)+4px)] border border-[var(--border)] bg-[var(--panel)] p-5 shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-[15px] font-semibold">{title}</h3>
          <IconButton label="Close" onClick={onClose}>
            <X size={16} />
          </IconButton>
        </div>
        <div className="text-[13px]">{children}</div>
        <div className="mt-5 flex justify-end gap-2">{actions}</div>
      </div>
    </div>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  if (!children) return null;
  return (
    <div role="alert" className="text-[12px] text-[var(--danger)] mt-1">
      {children}
    </div>
  );
}
