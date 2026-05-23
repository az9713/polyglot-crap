(function_definition
  declarator: (function_declarator
    declarator: (identifier) @fn.name)
  body: (compound_statement)) @fn.def

(function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier) @fn.name)
  body: (compound_statement)) @fn.def

(lambda_expression) @fn.def
