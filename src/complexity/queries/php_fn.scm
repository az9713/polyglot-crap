(function_definition
  name: (name) @fn.name
  body: (compound_statement)) @fn.def

(method_declaration
  name: (name) @fn.name
  body: (compound_statement)) @fn.def

(arrow_function) @fn.def

(anonymous_function) @fn.def
