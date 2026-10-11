// Free WhatsApp messages: a wa.me link opens WhatsApp with the text typed in,
// and the shopkeeper presses Send. No paid API needed.

/** Pakistani mobile number in international form (923001234567), or null. */
export function waNumber(phone: string): string | null {
  let d = phone.replace(/\D/g, '');
  if (d.startsWith('0092')) d = d.slice(2);
  // "+92 0300…": the trunk 0 written after the country code.
  if (d.startsWith('920') && d.length === 13) d = `92${d.slice(3)}`;
  if (d.startsWith('03') && d.length === 11) d = `92${d.slice(1)}`;
  if (d.startsWith('3') && d.length === 10) d = `92${d}`;
  return /^923\d{9}$/.test(d) ? d : null;
}

export function waLink(phone: string, text: string): string | null {
  const n = waNumber(phone);
  return n ? `https://wa.me/${n}?text=${encodeURIComponent(text)}` : null;
}
