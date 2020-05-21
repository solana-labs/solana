export function displayTimestamp(unixTimestamp: number): string {
  const expireDate = new Date(unixTimestamp * 1000);
  const dateString = new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric"
  }).format(expireDate);
  const timeString = new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "numeric",
    second: "numeric",
    hour12: false,
    timeZoneName: "long"
  }).format(expireDate);
  return `${dateString} at ${timeString}`;
}
