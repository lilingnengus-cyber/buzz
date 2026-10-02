import { useId, useRef, useState } from "react";

export function useOrderValidation() {
  const id = useId();
  const [errors, setErrors] = useState<string[]>([]);
  const targets = useRef<HTMLElement[]>([]);
  const clear = () => {
    for (const target of targets.current) {
      target.removeAttribute("aria-invalid");
      target.removeAttribute("aria-describedby");
    }
    targets.current = [];
    setErrors([]);
  };
  const focus = (target: HTMLElement) => {
    target.focus({ preventScroll: true });
    target.scrollIntoView({ block: "center", behavior: "auto" });
  };
  const validate = (form: HTMLFormElement) => {
    clear();
    const messages: string[] = [];
    for (const element of form.querySelectorAll<HTMLElement>("input, select, textarea, [data-order-required]")) {
      let target = element;
      let message = "";
      if (element.hasAttribute("data-order-required")) {
        const control = element.querySelector<HTMLButtonElement>("button");
        if (!control || control.matches(":disabled") || element.dataset.orderValue) continue;
        target = control;
        message = `请选择${element.dataset.orderRequired}`;
      } else if (element instanceof HTMLInputElement || element instanceof HTMLSelectElement || element instanceof HTMLTextAreaElement) {
        const blank = element.required && !element.value.trim();
        if (!element.willValidate || (element.validity.valid && !blank)) continue;
        const label = (element.getAttribute("aria-label") ?? element.labels?.[0]?.querySelector(":scope > span")?.textContent ?? element.labels?.[0]?.textContent ?? "此字段").trim().replace(/\s*\*$/, "");
        message = element.validity.valueMissing || blank ? `${element instanceof HTMLSelectElement ? "请选择" : "请填写"}${label}` : `请检查${label}的格式或取值范围`;
      }
      if (!message) continue;
      target.setAttribute("aria-invalid", "true");
      target.setAttribute("aria-describedby", `${id}-${messages.length}`);
      targets.current.push(target);
      messages.push(message);
    }
    setErrors(messages);
    if (targets.current[0]) focus(targets.current[0]);
    return messages.length === 0;
  };
  const summary = errors.length > 0 && <div className="entry-validation" role="alert">
    <strong>请补充或修正以下内容</strong>
    <ul>{errors.map((message, index) => <li key={index} id={`${id}-${index}`}>
      <button type="button" onClick={(event) => { event.stopPropagation(); if (targets.current[index]) focus(targets.current[index]); }}>{message}</button>
    </li>)}</ul>
  </div>;
  return { validate, clear, summary };
}
