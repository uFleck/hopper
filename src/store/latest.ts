/** A debounced caller. `schedule` restarts the timer; `flush` runs now; `cancel` drops the pending call. */
export function debounced(fn: () => void, ms: number) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const cancel = () => {
    if (timer) clearTimeout(timer);
    timer = null;
  };
  return {
    schedule() {
      cancel();
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, ms);
    },
    flush() {
      cancel();
      fn();
    },
    cancel,
    pending: () => timer !== null,
  };
}

/** Latest-wins: `begin()` returns a ticket; only the newest ticket is `current()`, so a slow earlier answer can be dropped. */
export function latestWins() {
  let seq = 0;
  return {
    begin() {
      const mine = ++seq;
      return { current: () => mine === seq };
    },
    invalidate() {
      seq++;
    },
  };
}
