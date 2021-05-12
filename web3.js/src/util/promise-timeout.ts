export function promiseTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
): Promise<T | null> {
  let timeoutId: ReturnType<typeof setTimeout>;
  const timeoutPromise: Promise<null> = new Promise(resolve => {
    timeoutId = setTimeout(() => resolve(null), timeoutMs);
  });

  return Promise.race([promise, timeoutPromise]).then((result: T | null) => {
    clearTimeout(timeoutId);
    return result;
  });
}
