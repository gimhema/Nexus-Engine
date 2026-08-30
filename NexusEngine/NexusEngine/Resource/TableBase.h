#pragma once

// Like CSV

#include <string>
#include <unordered_map>
#include <vector>

typedef int TKey;
typedef std::string TType;
typedef std::string TValue;

class TData
{
private:


public:

};

class TableBase
{
public:
	TableBase() = default;
	virtual ~TableBase() = default;
	virtual void LoadFromFile(const std::string& filePath) = 0;
	virtual void SaveToFile(const std::string& filePath) const = 0;

};