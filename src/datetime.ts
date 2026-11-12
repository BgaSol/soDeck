import { format, isValid, parseISO } from "date-fns";

export function date(value: number | string) {
  const time = typeof value === "string" ? parseISO(value) : value;
  return isValid(time) ? format(time, "yyyy/M/d HH:mm:ss") : "Invalid Date";
}

export const localTime = (value?: number) =>
  value === undefined ? "" : format(value, "yyyy-MM-dd'T'HH:mm");

export function parseLocalTime(value: string) {
  const time = parseISO(value);
  return isValid(time) ? time.getTime() : undefined;
}
