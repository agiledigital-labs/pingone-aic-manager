// Library body for the String.prototype.normalize probe. Uploaded as a LIBRARY
// script (evaluatorVersion 2.0) and required by string-normalize.script.js.
// Verifies NFD accent folding works at library scope -- the pattern lib-idr
// relies on: normalize('NFD') then strip combining marks U+0300-U+036F.
// Safe to delete.
function fold(s) {
  return s.normalize('NFD').replace(/[̀-ͯ]/g, '');
}
exports.foldedEacute = fold('José');                      // expect "Jose"
exports.foldedStacked = fold('Nguyễn');                   // expect "Nguyen"
exports.nfcLength = 'é'.normalize('NFC').length;         // expect 1
