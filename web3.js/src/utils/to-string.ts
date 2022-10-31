export default function toString(value: any): string {
  if (value instanceof Error) {
    return value.message;
  }

  if (typeof value === 'object') {
    return JSON.stringify(value);
  }

  return String(value);
}
