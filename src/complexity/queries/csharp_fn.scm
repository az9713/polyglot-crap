(method_declaration
  name: (identifier) @fn.name
  body: (_)) @fn.def

(constructor_declaration
  name: (identifier) @fn.name
  body: (block)) @fn.def

(local_function_statement
  name: (identifier) @fn.name
  body: (_)) @fn.def

(lambda_expression) @fn.def
