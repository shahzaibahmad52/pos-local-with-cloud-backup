// Till text for selling while the main PC can't be reached.

export const offEn = {
  offSelling: 'Main PC not reachable — selling from this till’s copy',
  offWaiting: '{n} offline sales waiting to send',
  offSent: 'Offline sales sent to the main PC.',
  offRefused: 'The main PC refused offline receipt {no} ({amount}): {reason}. Tell the owner.',
  offDismiss: 'Noted',
  offNoKhata: 'Khata can’t be used while the main PC is unreachable.',
  offNoCopy: 'This till has no product list yet, so it can’t sell until the main PC is back.',
  offDiscount: 'Discounts above {max}% need the main PC.',
  offNeedsMainPc: 'This needs a manager PIN, which the main PC checks. Wait until it’s back.',
  offQueueBlocks: 'Wait until the offline sales are sent before closing the shift or signing out.',
  offReceiptNote: 'Saved on this till; it will be sent to the main PC when the network is back.',
  offCopyAge: 'Product list from {time}',
  offSignInAgain: 'The main PC needs you to sign in again before it takes the {n} offline sales. They are kept safe on this till: sign out and sign back in.',
};

export const offUr: Record<keyof typeof offEn, string> = {
  offSelling: 'مین کمپیوٹر سے رابطہ نہیں — اس ٹل کی کاپی سے سیل جاری',
  offWaiting: '{n} آف لائن سیل بھیجنا باقی',
  offSent: 'آف لائن سیل مین کمپیوٹر کو بھیج دی گئیں۔',
  offRefused: 'مین کمپیوٹر نے آف لائن رسید {no} ({amount}) قبول نہیں کی: {reason}۔ مالک کو بتائیں۔',
  offDismiss: 'ٹھیک ہے',
  offNoKhata: 'مین کمپیوٹر سے رابطہ نہ ہو تو کھاتہ استعمال نہیں ہو سکتا۔',
  offNoCopy: 'اس ٹل پر ابھی پروڈکٹ لسٹ نہیں، اس لیے مین کمپیوٹر واپس آنے تک سیل نہیں ہو سکتی۔',
  offDiscount: '{max}% سے زیادہ رعایت کے لیے مین کمپیوٹر ضروری ہے۔',
  offNeedsMainPc: 'اس کے لیے مینیجر PIN چاہیے جو مین کمپیوٹر چیک کرتا ہے۔ اس کے واپس آنے کا انتظار کریں۔',
  offQueueBlocks: 'شفٹ بند کرنے یا سائن آؤٹ سے پہلے آف لائن سیل بھیجے جانے کا انتظار کریں۔',
  offReceiptNote: 'اس ٹل پر محفوظ ہے؛ نیٹ ورک واپس آنے پر مین کمپیوٹر کو بھیج دی جائے گی۔',
  offCopyAge: 'پروڈکٹ لسٹ {time} کی',
  offSignInAgain: 'مین کمپیوٹر {n} آف لائن سیل لینے سے پہلے دوبارہ سائن اِن چاہتا ہے۔ یہ اس ٹل پر محفوظ ہیں: سائن آؤٹ کر کے دوبارہ سائن اِن کریں۔',
};
