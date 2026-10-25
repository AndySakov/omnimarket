// The contract's wire forms (D91), shared by the contract test and the hostile-value generator.

/** Fields holding amounts, prices, USD values or percentages: decimal strings on the wire. */
export const decimalField = /(^|_)(usd|pct|amount|price|supply|native|bought|returned|spent|quote)$|^amount_|_amount_|^pct_/
