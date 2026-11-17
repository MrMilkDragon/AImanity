export function windowsReleaseSigning() {
  if (process.platform !== "win32")
    throw Error("Trusted Windows signing requires the Windows certificate store and SignTool");
  const thumbprint = process.env.AIMANITY_WINDOWS_CERT_THUMBPRINT?.trim();
  const timestampUrl = process.env.AIMANITY_WINDOWS_TIMESTAMP_URL?.trim();
  if (!thumbprint || !/^[a-f0-9]{40}$/i.test(thumbprint) || !timestampUrl)
    throw Error("Windows certificate thumbprint and RFC 3161 timestamp URL are required");
  return { thumbprint, timestampUrl };
}
