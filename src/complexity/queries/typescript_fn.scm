(function_declaration
  name: (identifier) @fn.name) @fn.def

(function_expression
  name: (identifier)? @fn.name) @fn.def

(arrow_function) @fn.def

(method_definition
  name: (property_identifier) @fn.name) @fn.def

(method_signature
  name: (property_identifier) @fn.name) @fn.def
