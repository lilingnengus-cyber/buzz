import React from "react";
const DraftContext = React.createContext({
  markDirty: () => {},
  saved: () => {},
  setBusy: (_busy: boolean) => {},
  discard: (action: () => void) => action(),
});
export const useCrmDraft = () => React.useContext(DraftContext);
export function CrmDrawer({
  children,
  title,
  onClose,
}: {
  children: React.ReactNode;
  title: string;
  onClose: () => void;
}) {
  const ref = React.useRef<HTMLDialogElement>(null);
  const [dirty, setDirty] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [pending, setPending] = React.useState<(() => void) | null>(null);
  const discard = (action: () => void) => {
    if (busy) return;
    if (dirty) {
      setPending(() => action);
      return;
    }
    action();
  };
  const close = () => discard(onClose);
  React.useEffect(() => {
    const dialog = ref.current;
    const previousOverflow = document.body.style.overflow;
    dialog?.showModal();
    document.body.style.overflow = "hidden";
    return () => {
      dialog?.close();
      document.body.style.overflow = previousOverflow;
    };
  }, []);
  React.useEffect(() => {
    const guard = (event: BeforeUnloadEvent) => {
      if (dirty || busy) {
        event.preventDefault();
        event.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [dirty, busy]);
  return (
    <DraftContext.Provider
      value={{
        markDirty: () => setDirty(true),
        saved: () => setDirty(false),
        setBusy,
        discard,
      }}
    >
      <dialog
        ref={ref}
        className="crm-drawer"
        aria-label={title}
        onCancel={(event) => {
          event.preventDefault();
          close();
        }}
        onClick={(event) => {
          if (event.target === event.currentTarget) close();
        }}
        onClickCapture={(event) => {
          const anchor = (
            event.target as HTMLElement
          ).closest<HTMLAnchorElement>("a[href]");
          if (anchor && (dirty || busy)) {
            event.preventDefault();
            discard(() => window.location.assign(anchor.href));
          }
        }}
      >
        <div className="crm-drawer-content">
          <header className="crm-drawer-heading">
            <strong>{title}</strong>
            <button aria-label="关闭商机弹窗" disabled={busy} onClick={close}>
              关闭 ×
            </button>
          </header>
          {children}
        </div>
        {pending && (
          <DiscardPrompt
            onCancel={() => setPending(null)}
            onDiscard={() => {
              setDirty(false);
              setPending(null);
              pending();
            }}
          />
        )}
      </dialog>
    </DraftContext.Provider>
  );
}

function DiscardPrompt({
  onCancel,
  onDiscard,
}: {
  onCancel: () => void;
  onDiscard: () => void;
}) {
  const ref = React.useRef<HTMLDialogElement>(null);
  React.useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className="crm-discard-prompt"
      aria-label="放弃未保存修改"
      onCancel={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onCancel();
      }}
      onClick={(e) => e.stopPropagation()}
    >
      <h2>有未保存的修改</h2>
      <p>离开将放弃这些修改，是否继续？</p>
      <div className="crm-discard-actions">
        <button autoFocus onClick={onCancel}>
          继续编辑
        </button>
        <button onClick={onDiscard}>放弃修改</button>
      </div>
    </dialog>
  );
}
