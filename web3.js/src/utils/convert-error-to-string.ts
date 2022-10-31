export default function convertErrorToString(error: any): string {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === 'object') {
    return JSON.stringify(error);
  }

  return String(error);
}
