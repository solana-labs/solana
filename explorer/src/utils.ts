export function findGetParameter(parameterName: string): string | null {
  let result = null,
    tmp = [];
  window.location.search
    .substr(1)
    .split("&")
    .forEach(function(item) {
      tmp = item.split("=");
      if (tmp[0] === parameterName) {
        if (tmp.length === 2) {
          result = decodeURIComponent(tmp[1]);
        } else if (tmp.length === 1) {
          result = "";
        }
      }
    });
  return result;
}

export function findPathSegment(pathName: string): string | null {
  const segments = window.location.pathname.substr(1).split("/");
  if (segments.length < 2) return null;

  // remove all but last two segments
  segments.splice(0, segments.length - 2);

  if (segments[0] === pathName) {
    return segments[1];
  }

  return null;
}

export function assertUnreachable(x: never): never {
  throw new Error("Unreachable!");
}
