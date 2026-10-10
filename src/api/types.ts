// Shapes returned by the store server (crates/pos-server). Money is paisa.

export type Language = 'en' | 'ur';
export type ReceiptLanguage = 'en' | 'ur' | 'both';

export interface User {
  id: string;
  name: string;
  role: 'admin' | 'cashier';
  canApprove: boolean;
  language: Language;
}

export interface LoginTile {
  id: string;
  name: string;
  role: 'admin' | 'cashier';
  language: Language;
  canApprove: boolean;
}

export interface Unit {
  id: string;
  name: string;
  nameUr: string | null;
  baseQty: number;
  price: number;
}

export interface Product {
  id: string;
  name: string;
  genericName: string | null;
  company: string | null;
  category: string | null;
  kind: 'medicine' | 'general';
  sellMode: 'unit' | 'weight';
  baseUnit: string;
  pricePerKg: number | null;
  taxRateBp: number;
  rxRequired: boolean;
  controlled: boolean;
  coldChain: boolean;
  rack: string | null;
  units: Unit[];
  stockBase: number;
  nextBatchNo: string | null;
  nextExpiry: string | null;
}

export interface BarcodeHit {
  product: Product;
  unitId: string | null;
}

export interface StoreHeader {
  code: string;
  name: string;
  address: string | null;
  phone: string | null;
  ntn: string | null;
  drugLicense: string | null;
  receiptFooter: string | null;
  receiptLanguage: ReceiptLanguage;
}

export interface Health {
  ok: boolean;
  setUp: boolean;
  store: StoreHeader | null;
  version: string;
}

export interface Shift {
  id: string;
  tillCode: string;
  userId: string;
  userName: string;
  openedAt: string;
  openingCash: number;
}

export interface Customer {
  id: string;
  name: string;
  phone: string | null;
  creditLimit: number;
  balance: number;
  receiptLanguage: ReceiptLanguage | null;
}

export type PaymentMethod = 'cash' | 'card' | 'jazzcash' | 'easypaisa' | 'khata';

export interface SaleLineInput {
  productId: string;
  unitId?: string;
  qty?: number;
  grams?: number;
  amount?: number;
}

export interface SaleInput {
  tillCode: string;
  customerId?: string;
  lines: SaleLineInput[];
  discountBp: number;
  payments: { method: PaymentMethod; amount: number }[];
  clientRef: string;
  approval?: ApprovalInput;
}

export interface ReceiptLine {
  productName: string;
  unitName: string;
  sellMode: 'unit' | 'weight';
  qty: number;
  unitPrice: number;
  lineTotal: number;
  taxIncluded: number;
  batchNo: string | null;
}

export interface Receipt {
  id: string;
  receiptNo: string;
  createdAt: string;
  tillCode: string;
  cashierName: string;
  customerName: string | null;
  customerReceiptLanguage: ReceiptLanguage | null;
  lines: ReceiptLine[];
  itemCount: number;
  subtotal: number;
  discount: number;
  taxIncluded: number;
  total: number;
  paid: number;
  changeDue: number;
  payments: { method: PaymentMethod; amount: number }[];
  khataBalance: number | null;
  store: StoreHeader;
}

export interface ApprovalInput {
  userId: string;
  secret: string;
}

export interface ApprovalRules {
  cashierMaxDiscountBp: number;
  refundApprovalAbove: number;
  removeItem: boolean;
  cancelBill: boolean;
  cashOut: boolean;
  noSaleDrawer: boolean;
}

export type TillAction = 'remove_item' | 'cancel_bill' | 'no_sale_drawer';

export interface ActivityEntry {
  id: string;
  createdAt: string;
  tillCode: string | null;
  userName: string | null;
  action: string;
  details: Record<string, unknown> | null;
  approvedByName: string | null;
  flagged: boolean;
}

export interface ReturnableLine {
  saleLineId: string;
  productName: string;
  unitName: string;
  sellMode: 'unit' | 'weight';
  qty: number;
  returnedQty: number;
  unitPrice: number;
  lineTotal: number;
  batchNo: string | null;
  batchExpiry: string | null;
  blocked: string | null;
}

export interface SaleForReturn {
  id: string;
  receiptNo: string;
  createdAt: string;
  cashierName: string;
  customerId: string | null;
  customerName: string | null;
  subtotal: number;
  total: number;
  refundedSoFar: number;
  paymentMethods: PaymentMethod[];
  lines: ReturnableLine[];
}

export type ReturnCondition = 'restock' | 'damaged' | 'expired';
export type RefundMethod = 'cash' | 'khata' | 'card' | 'jazzcash' | 'easypaisa';

export interface ReturnInput {
  saleId: string;
  tillCode: string;
  lines: { saleLineId: string; qty: number; condition: ReturnCondition; reason?: string }[];
  refundMethod: RefundMethod;
  clientRef: string;
  approval?: ApprovalInput;
}

export interface ReturnSlip {
  id: string;
  returnNo: string;
  saleReceiptNo: string;
  createdAt: string;
  tillCode: string;
  cashierName: string;
  customerName: string | null;
  lines: { productName: string; unitName: string; sellMode: 'unit' | 'weight'; qty: number; refund: number; condition: ReturnCondition }[];
  refundTotal: number;
  refundMethod: RefundMethod;
  approvedByName: string | null;
  khataBalance: number | null;
  store: StoreHeader;
}

export interface ShiftSummary {
  id: string;
  tillCode: string;
  cashierName: string;
  openedAt: string;
  closedAt: string | null;
  status: 'open' | 'closed';
  openingCash: number;
  billCount: number;
  salesTotal: number;
  cashSales: number;
  cashRefunds: number;
  refundsTotal: number;
  returnsCount: number;
  cashIn: number;
  cashOut: number;
  otherPayments: Record<string, number>;
  expectedCash: number | null;
  countedCash: number | null;
  difference: number | null;
  countAttempts: number;
  note: string | null;
  store: StoreHeader;
}
