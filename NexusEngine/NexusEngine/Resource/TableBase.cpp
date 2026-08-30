#include "TableBase.h"

#include <stdexcept>


namespace TableHelper
{
	TTypedValue ParseValue(const TData& data)
	{
		const TType& type = data.GetType();
		const TValue& value = data.GetValue();

		if (type == "int")
			return std::stoi(value);
		if (type == "float")
			return std::stof(value);
		if (type == "bool")
			return value == "true" || value == "1";
		if (type == "string")
			return value;

		throw std::runtime_error("TableHelper::ParseValue - unknown TType: " + type);
	}
}
