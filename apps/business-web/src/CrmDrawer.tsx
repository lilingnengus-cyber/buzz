import React from "react";
const DraftContext = React.createContext({
  markDirty: () => {},
  saved: () => {},
  setBusy: (_busy: boolean) => {},
  confirmDiscard: (): boolean => true,
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
  const confirmDiscard = () => {
    if (busy) return false;
    if (dirty && !window.confirm("有未保存的修改，确定放弃并离开吗？"))
      return false;
    setDirty(false);
    return true;
  };
  const close = () => {
    if (confirmDiscard()) onClose();
  };
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
        confirmDiscard,
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
          if (
            (event.target as HTMLElement).closest("a[href]") &&
            !confirmDiscard()
          )
            event.preventDefault();
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
      </dialog>
    </DraftContext.Provider>
  );
}
