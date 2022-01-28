import { ReactNode } from "react";

export function Downloadable({
  data,
  filename,
  children,
}: {
  data: string;
  filename: string;
  children: ReactNode;
}) {
  const handleClick = async () => {
    const blob = new Blob([Buffer.from(data, "base64")]);
    const fileDownloadUrl = URL.createObjectURL(blob);
    const tempLink = document.createElement("a");
    tempLink.href = fileDownloadUrl;
    tempLink.setAttribute("download", filename);
    tempLink.click();
  };

  return (
    <>
      <span className="fe fe-download c-pointer me-2" onClick={handleClick} />
      {children}
    </>
  );
}
