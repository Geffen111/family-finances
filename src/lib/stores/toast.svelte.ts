// Shared toast state. A single <Toast /> lives in the root layout and every
// page calls showToast() instead of carrying its own state/markup/styles.
type ToastType = "success" | "error";

let message = $state("");
let kind = $state<ToastType>("success");
let visible = $state(false);
let timer: ReturnType<typeof setTimeout> | undefined;

export const toast = {
  get message() {
    return message;
  },
  get kind() {
    return kind;
  },
  get visible() {
    return visible;
  },
};

export function showToast(msg: string, type: ToastType = "success") {
  message = msg;
  kind = type;
  visible = true;
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => {
    visible = false;
  }, 4000);
}
