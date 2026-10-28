import { invoke, type CookieDto } from "../../ipc";

export const listCookies = (): Promise<CookieDto[]> => invoke("cookies_list");
export const clearCookies = (): Promise<void> => invoke("cookies_clear");
