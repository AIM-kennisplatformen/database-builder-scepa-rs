export const TEXT_REGEX = "^[A-Za-zÀ-ÖØ-öø-ÿ0-9\\s.,:;'\"!?()&-]+$";

export function isValidField(value, regex) {
  // No regex configured for this field means there's nothing to validate against.
  if (!regex) return false;

  // Empty values are left to a separate "required" check, not format validation.
  if (value === "" || value == null) return true;

  try {
    const pattern = regex instanceof RegExp ? regex : new RegExp(regex);
    return pattern.test(value);
  } catch (err) {
    return false;
  }
}
