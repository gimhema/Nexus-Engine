#pragma once

// Like CSV

#include <string>
#include <unordered_map>
#include <variant>
#include <vector>


typedef int TKey;
typedef std::string TType;
typedef std::string TValue;

class TData;

namespace TableHelper
{
	using TTypedValue = std::variant<int, float, bool, std::string>;

	// TData::GetType() 문자열("int"/"float"/"bool"/"string")에 맞춰 GetValue()를 파싱해서 반환
	TTypedValue ParseValue(const TData& data);
}

class TData
{
private:
	TKey m_key;
	TType m_type;
	TValue m_value;

public:
	TData() = default;
	TData(TKey key, TType type, TValue value)
		: m_key(key), m_type(type), m_value(value) {
	}
	TKey GetKey() const { return m_key; }
	TType GetType() const { return m_type; }
	TValue GetValue() const { return m_value; }
	void SetKey(TKey key) { m_key = key; }
	void SetType(TType type) { m_type = type; }
	void SetValue(TValue value) { m_value = value; }

};

class TableBase
{
private:
	std::unordered_map<TKey, TData> m_dataMap;

public:
	TableBase() = default;
	virtual ~TableBase() = default;
	virtual void LoadFromFile(const std::string& filePath) = 0;
	virtual void SaveToFile(const std::string& filePath) const = 0;


};