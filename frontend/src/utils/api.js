export const API_BASE_URL = `${import.meta.env.BASE_URL}api`;

export function apiUrl(path = "") {
  if (!path) return API_BASE_URL;
  return `${API_BASE_URL}${path.startsWith("/") ? path : `/${path}`}`;
}
